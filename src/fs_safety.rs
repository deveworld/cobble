use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub(crate) fn write_file_atomic(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let (mut file, temp_path) = create_temp_output_file(path)?;
    let result = (|| {
        file.write_all(contents.as_ref())?;
        file.flush()?;
        drop(file);
        replace_with_temp_file(&temp_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[allow(dead_code)]
pub(crate) fn write_file_atomic_with_permissions(
    path: &Path,
    contents: impl AsRef<[u8]>,
    permissions: fs::Permissions,
) -> io::Result<()> {
    let (mut file, temp_path) = create_temp_output_file_with_permissions(path, Some(&permissions))?;
    let result = (|| {
        #[cfg(not(unix))]
        fs::set_permissions(&temp_path, permissions.clone())?;
        file.write_all(contents.as_ref())?;
        file.flush()?;
        drop(file);
        replace_with_temp_file(&temp_path, path)?;
        fs::set_permissions(path, permissions)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[allow(dead_code)]
pub(crate) fn copy_file_atomic(source: &Path, target: &Path) -> io::Result<u64> {
    let mut source_file = File::open(source)?;
    let (mut target_file, temp_path) = create_temp_output_file(target)?;
    let result = (|| {
        let copied = io::copy(&mut source_file, &mut target_file)?;
        target_file.flush()?;
        drop(target_file);
        replace_with_temp_file(&temp_path, target)?;
        Ok(copied)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

pub(crate) fn create_temp_output_file(path: &Path) -> io::Result<(File, PathBuf)> {
    create_temp_output_file_with_permissions(path, None)
}

fn create_temp_output_file_with_permissions(
    path: &Path,
    permissions: Option<&fs::Permissions>,
) -> io::Result<(File, PathBuf)> {
    #[cfg(not(unix))]
    let _ = permissions;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    for attempt in 0..32 {
        let temp_path = parent.join(format!(
            ".{}.cobble-write-{}-{}-{}.tmp",
            name,
            std::process::id(),
            timestamp_nanos(),
            attempt
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if let Some(permissions) = permissions {
            options.mode(permissions.mode() & 0o777);
        }
        match options.open(&temp_path) {
            Ok(file) => return Ok((file, temp_path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "could not allocate temporary output file next to {}",
            path.display()
        ),
    ))
}

pub(crate) fn replace_with_temp_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    match fs::rename(temp_path, target_path) {
        Ok(()) => Ok(()),
        Err(rename_error) if target_path.exists() => {
            fs::remove_file(target_path)?;
            fs::rename(temp_path, target_path).map_err(|second_error| {
                io::Error::new(
                    second_error.kind(),
                    format!(
                        "failed to replace {} after removing existing file: {}; original rename error: {}",
                        target_path.display(),
                        second_error,
                        rename_error
                    ),
                )
            })
        }
        Err(error) => Err(error),
    }
}

fn timestamp_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}
