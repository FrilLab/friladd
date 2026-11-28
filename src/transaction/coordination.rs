//! Coordinate on the project directory's identity without creating a file.

#[cfg(unix)]
pub(super) struct Guard {
    _directory: std::fs::File,
}

#[cfg(unix)]
impl Guard {
    pub(super) fn acquire(project: &std::path::Path) -> std::io::Result<Self> {
        let directory = std::fs::File::open(project)?;
        fs2::FileExt::try_lock_exclusive(&directory)?;
        Ok(Self {
            _directory: directory,
        })
    }
}

#[cfg(windows)]
pub(super) use windows::Guard;

#[cfg(windows)]
mod windows {
    use std::{
        fs::OpenOptions,
        io,
        marker::PhantomData,
        mem::{MaybeUninit, size_of},
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        path::Path,
        ptr,
        rc::Rc,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
        },
        System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
    };

    pub(in crate::transaction) struct Guard {
        mutex: HANDLE,
        // Win32 requires the acquiring thread to release the mutex.
        _thread: PhantomData<Rc<()>>,
    }

    impl Guard {
        pub(in crate::transaction) fn acquire(project: &Path) -> io::Result<Self> {
            let directory = OpenOptions::new()
                .access_mode(0)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(project)?;
            let mut id = MaybeUninit::<FILE_ID_INFO>::uninit();
            // The handle remains open for the call and id points to the full output buffer.
            let read = unsafe {
                GetFileInformationByHandleEx(
                    directory.as_raw_handle(),
                    FileIdInfo,
                    id.as_mut_ptr().cast(),
                    size_of::<FILE_ID_INFO>() as u32,
                )
            };
            if read == 0 {
                return Err(io::Error::last_os_error());
            }
            // Success initializes exactly one FILE_ID_INFO.
            let id = unsafe { id.assume_init() };
            let file_id = u128::from_le_bytes(id.FileId.Identifier);
            let name = format!(
                "Global\\FrilAdd.Mutation.{:016x}.{file_id:032x}",
                id.VolumeSerialNumber
            );
            let name: Vec<_> = name.encode_utf16().chain([0]).collect();
            // Use the default ACL. A caller denied access fails closed across logon sessions.
            let mutex = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
            if mutex.is_null() {
                return Err(io::Error::last_os_error());
            }
            match unsafe { WaitForSingleObject(mutex, 0) } {
                WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self {
                    mutex,
                    _thread: PhantomData,
                }),
                status => {
                    let error = if status == WAIT_TIMEOUT {
                        io::Error::new(io::ErrorKind::WouldBlock, "another mutation is running")
                    } else {
                        io::Error::last_os_error()
                    };
                    unsafe { CloseHandle(mutex) };
                    Err(error)
                }
            }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // This guard owns the handle and cannot move to another thread.
            unsafe {
                ReleaseMutex(self.mutex);
                CloseHandle(self.mutex);
            }
        }
    }
}
