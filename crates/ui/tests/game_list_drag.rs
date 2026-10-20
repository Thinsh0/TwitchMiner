//! Reordering the game list by dragging its handle (headless, Slint testing backend).

use std::cell::RefCell;
use std::rc::Rc;

use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, ModelRc, VecModel};
use tm_ui::{Bridge, Conn, GameItem, MainWindow, Screen};

fn press_drag_release(ui: &MainWindow, from: LogicalPosition, dy: f32) {
    let w = ui.window();
    w.dispatch_event(WindowEvent::PointerMoved { position: from });
    w.dispatch_event(WindowEvent::PointerPressed {
        position: from,
        button: PointerEventButton::Left,
    });
    let steps = 10;
    for i in 1..=steps {
        let position = LogicalPosition::new(from.x, from.y + dy * i as f32 / steps as f32);
        w.dispatch_event(WindowEvent::PointerMoved { position });
    }
    w.dispatch_event(WindowEvent::PointerReleased {
        position: LogicalPosition::new(from.x, from.y + dy),
        button: PointerEventButton::Left,
    });
}

#[test]
fn dragging_a_game_handle_moves_it() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    ui.window()
        .set_size(slint::LogicalSize::new(1500.0, 1000.0));
    let b = ui.global::<Bridge>();
    b.set_conn(Conn::Ready);
    b.set_screen(Screen::Drops);
    b.set_drops_enabled(true);
    let games: Vec<GameItem> = ["Alpha", "Bravo", "Charlie", "Delta"]
        .into_iter()
        .map(|n| GameItem {
            name: n.into(),
            campaigns: 0,
        })
        .collect();
    b.set_games(ModelRc::new(VecModel::from(games)));
    let moved: Rc<RefCell<Vec<(String, i32, i32)>>> = Rc::default();
    let m = moved.clone();
    b.on_list_move(move |key, from, to| m.borrow_mut().push((key.into(), from, to)));
    ui.show().unwrap();

    // The row (list item) of "Alpha": its handle is at the left edge.
    let row = ElementHandle::find_by_accessible_label(&ui, "Alpha")
        .find(|e| e.size().width > 200.0)
        .expect("row of Alpha");
    let pos = row.absolute_position();
    let handle = LogicalPosition::new(pos.x + 14.0, pos.y + row.size().height / 2.0);

    // Two rows down (rows are 40 px apart).
    press_drag_release(&ui, handle, 82.0);
    assert_eq!(*moved.borrow(), vec![("games".to_string(), 0, 2)]);

    // A click on the handle without moving does nothing.
    moved.borrow_mut().clear();
    press_drag_release(&ui, handle, 0.0);
    assert!(moved.borrow().is_empty());
}
