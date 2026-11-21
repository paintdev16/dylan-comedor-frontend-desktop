#[cfg(windows)]
mod platform {
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
        let mutex_name = if crate::infra::veltrix::IS_BETA {
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
}

#[cfg(not(windows))]
mod platform {
    use std::{
        fs::{self, File, OpenOptions, TryLockError},
        path::PathBuf,
    };

    pub struct Instance(#[allow(dead_code)] File);

    fn lock_path() -> PathBuf {
        let filename = if crate::infra::veltrix::IS_BETA {
            "dylan-beta.lock"
        } else {
            "dylan.lock"
        };
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join(filename)
    }

    pub fn claim() -> std::io::Result<Option<Instance>> {
        let path = lock_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Instance(file))),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }
}

pub use platform::claim;
