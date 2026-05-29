#[cfg(target_os = "android")]
use jni::objects::{GlobalRef, JObject, JString, JValue};
#[cfg(target_os = "android")]
use jni::{JNIEnv, JavaVM};
#[cfg(target_os = "android")]
use jni::sys::{JNI_GetCreatedJavaVMs, JavaVM as SysJavaVM};
#[cfg(target_os = "android")]
use std::sync::Mutex;

#[cfg(target_os = "android")]
lazy_static::lazy_static! {
    static ref WAKE_LOCK: Mutex<Option<GlobalRef>> = Mutex::new(None);
}

#[cfg(target_os = "android")]
fn get_vm() -> Result<JavaVM, String> {
    type GetCreatedJavaVMs = unsafe extern "C" fn(*mut *mut SysJavaVM, jni::sys::jsize, *mut jni::sys::jsize) -> std::os::raw::c_int;
    unsafe {
        let lib = libc::dlopen(b"libart.so\0".as_ptr() as *const _, libc::RTLD_NOW);
        if lib.is_null() {
            return Err("Failed to load libart.so".into());
        }
        let sym = libc::dlsym(lib, b"JNI_GetCreatedJavaVMs\0".as_ptr() as *const _);
        if sym.is_null() {
            return Err("Failed to find JNI_GetCreatedJavaVMs".into());
        }
        let func: GetCreatedJavaVMs = std::mem::transmute(sym);
        let mut vms: [*mut SysJavaVM; 1] = [std::ptr::null_mut()];
        let mut num_vms = 0;
        let res = func(vms.as_mut_ptr(), 1, &mut num_vms);
        if res != jni::sys::JNI_OK || num_vms == 0 {
            return Err("Failed to get JavaVM".into());
        }
        JavaVM::from_raw(vms[0] as *mut _).map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "android")]
pub fn acquire_wakelock() -> Result<(), String> {
    let vm = get_vm()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    
    let activity_thread = env.find_class("android/app/ActivityThread").map_err(|e| e.to_string())?;
    let context_obj = env.call_static_method(activity_thread, "currentApplication", "()Landroid/app/Application;", &[])
        .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;

    let power_service_name = env.new_string("power").map_err(|e| e.to_string())?;
    
    let power_manager = env
        .call_method(
            &context_obj,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[JValue::Object(&power_service_name.into())],
        )
        .map_err(|e| e.to_string())?
        .l()
        .map_err(|e| e.to_string())?;

    let tag = env.new_string("TwitchMiner::BackgroundMiner").map_err(|e| e.to_string())?;

    let wake_lock = env
        .call_method(
            &power_manager,
            "newWakeLock",
            "(ILjava/lang/String;)Landroid/os/PowerManager$WakeLock;",
            &[JValue::Int(1), JValue::Object(&tag.into())],
        )
        .map_err(|e| e.to_string())?
        .l()
        .map_err(|e| e.to_string())?;

    env.call_method(&wake_lock, "acquire", "()V", &[])
        .map_err(|e| e.to_string())?;

    let global_ref = env.new_global_ref(wake_lock).map_err(|e| e.to_string())?;
    *WAKE_LOCK.lock().unwrap() = Some(global_ref);

    log::info!("Wakelock acquired.");
    Ok(())
}

#[cfg(not(target_os = "android"))]
pub fn acquire_wakelock() -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "android")]
pub fn release_wakelock() -> Result<(), String> {
    let mut wl = WAKE_LOCK.lock().unwrap();
    if let Some(wake_lock_ref) = wl.take() {
        let vm = get_vm()?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        
        let is_held = env.call_method(wake_lock_ref.as_obj(), "isHeld", "()Z", &[])
            .map_err(|e| e.to_string())?
            .z()
            .unwrap_or(false);
            
        if is_held {
            env.call_method(wake_lock_ref.as_obj(), "release", "()V", &[])
                .map_err(|e| e.to_string())?;
            log::info!("Wakelock released.");
        }
    }
    Ok(())
}

#[cfg(not(target_os = "android"))]
pub fn release_wakelock() -> Result<(), String> {
    Ok(())
}
