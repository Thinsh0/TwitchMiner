use jni::{JavaVM, JNIEnv};
use jni::sys::{JNI_GetCreatedJavaVMs, JavaVM as SysJavaVM};
use jni::objects::JValue;

pub fn get_vm() -> Result<JavaVM, String> {
    let mut vms: [*mut SysJavaVM; 1] = [std::ptr::null_mut()];
    let mut num_vms = 0;
    let res = unsafe { JNI_GetCreatedJavaVMs(vms.as_mut_ptr(), 1, &mut num_vms) };
    if res != jni::sys::JNI_OK || num_vms == 0 {
        return Err("Failed to get JavaVM".into());
    }
    unsafe { JavaVM::from_raw(vms[0] as *mut _) }.map_err(|e| e.to_string())
}

pub fn test() {
    let vm = get_vm().unwrap();
    let mut env = vm.attach_current_thread().unwrap();
    let activity_thread = env.find_class("android/app/ActivityThread").unwrap();
    let app_context = env.call_static_method(activity_thread, "currentApplication", "()Landroid/app/Application;", &[]).unwrap().l().unwrap();
    println!("App context: {:?}", app_context);
}
