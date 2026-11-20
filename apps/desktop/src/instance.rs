#![cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    System::Threading::CreateMutexW,
};

pub struct Instance(HANDLE);

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub fn claim() -> std::io::Result<Option<Instance>> {
    let mutex_name = if crate::updater::IS_BETA {
        "Local\\Dylan.Test.Veltrix.Desktop"
    } else {
        "Local\\Dylan.Desktop"
    };
    let name: Vec<u16> = mutex_name.encode_utf16().chain([0]).collect();
    let (handle, error) = unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        (handle, GetLastError())
    };
    if handle.is_null() {
        return Err(std::io::Error::from_raw_os_error(error as i32));
    }
    let instance = Instance(handle);
    Ok((error != ERROR_ALREADY_EXISTS).then_some(instance))
}
