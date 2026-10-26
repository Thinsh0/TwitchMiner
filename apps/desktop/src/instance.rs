//! One desktop application per user and data directory: a second launch asks the
//! running one to show its window, then exits.

use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

const SHOW: &str = "twitchminer-show";
const OK: &str = "twitchminer-ok";

/// Local port of this user and data directory (outside the usual service ports).
fn port() -> u16 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::env::var("TM_DATA_DIR")
        .unwrap_or_default()
        .hash(&mut h);
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
        .hash(&mut h);
    40000 + (h.finish() % 20000) as u16
}

pub enum Instance {
    /// This is the first one: listen for later launches (`None` if the port is taken
    /// by something else).
    First(Option<TcpListener>),
    /// Another one is running and was asked to show its window.
    Other,
}

pub fn acquire() -> Instance {
    acquire_on(port())
}

/// Like [`acquire`], once the previous version (which started this one) has exited:
/// asking it to show its window would end this launch.
pub fn acquire_after_update() -> Instance {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port()));
    for _ in 0..60 {
        if let Ok(listener) = TcpListener::bind(addr) {
            return Instance::First(Some(listener));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    acquire()
}

fn acquire_on(port: u16) -> Instance {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    if let Ok(listener) = TcpListener::bind(addr) {
        return Instance::First(Some(listener));
    }
    // The port is taken: by us, if it answers our protocol.
    let asked = (|| -> std::io::Result<bool> {
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(1))?;
        s.set_read_timeout(Some(Duration::from_secs(2)))?;
        writeln!(s, "{SHOW}")?;
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line)?;
        Ok(line.trim() == OK)
    })();
    match asked {
        Ok(true) => Instance::Other,
        _ => Instance::First(None),
    }
}

/// Answers later launches (on a background thread): shows the window.
pub fn serve(listener: TcpListener, show: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("tm-instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() && line.trim() == SHOW {
                    let _ = writeln!(&stream, "{OK}");
                    show();
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn a_second_launch_shows_the_first_one() {
        // A free port of our own, so a running application is never involved.
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .unwrap();
        let Instance::First(Some(listener)) = acquire_on(port) else {
            panic!("first launch must own the port");
        };
        let shown = Arc::new(AtomicBool::new(false));
        let s = shown.clone();
        serve(listener, move || s.store(true, Ordering::SeqCst));
        assert!(matches!(acquire_on(port), Instance::Other));
        std::thread::sleep(Duration::from_millis(100));
        assert!(shown.load(Ordering::SeqCst));
    }

    #[test]
    fn a_port_used_by_something_else_is_not_us() {
        let foreign = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = foreign.local_addr().unwrap().port();
        // Accepts but never answers our protocol.
        std::thread::spawn(move || {
            let _keep = foreign.incoming().next();
            std::thread::sleep(Duration::from_secs(5));
        });
        assert!(matches!(acquire_on(port), Instance::First(None)));
    }
}
