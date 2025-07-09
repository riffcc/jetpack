// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.
// 
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

use std::fs;
use std::path::{Path};
use std::fs::ReadDir;
use std::os::unix::fs::PermissionsExt;
use std::process;
use std::io::Read;

// read a directory as per the normal rust way, but map any errors to strings
pub fn jet_read_dir(path: &Path) -> Result<ReadDir, String> {
    return fs::read_dir(path).map_err(
        |_x| format!("failed to read directory: {}", path.display())
    )
}

// call fn on each path in a subdirectory of the original path, each step is allowed
// to return an error to stop the walking.
pub fn path_walk<F>(path: &Path, mut with_each_path: F) -> Result<(), String> 
   where F: FnMut(&Path) -> Result<(), String> {
    let read_result = jet_read_dir(path);
    for entry in read_result.unwrap() {
        with_each_path(&entry.unwrap().path())?;
    }
    Ok(())
}

// open a file per the normal rust way, but map any errors to strings
pub fn jet_file_open(path: &Path) -> Result<std::fs::File, String> {
    return std::fs::File::open(path).map_err(
        |_x| format!("unable to open file: {}", path.display())
    );
}

pub fn read_local_file(path: &Path) -> Result<String,String> {
    let mut file = jet_file_open(path)?;
    let mut buffer = String::new();
    let read_result = file.read_to_string(&mut buffer);
    match read_result {
        Ok(_) => {},
        Err(x) => {
            return Err(format!("unable to read file: {}, {:?}", path.display(), x));
        }
    };
    return Ok(buffer.clone());
}

// get the last part of the file ignoring the directory part
pub fn path_basename_as_string(path: &Path) -> String {
    return path.file_name().unwrap().to_str().unwrap().to_string();
}

// get the last part of the file ignoring the directory part
pub fn path_as_string(path: &Path) -> String {
    return path.to_str().unwrap().to_string();
}

pub fn directory_as_string(path: &Path) -> String {
    return path.parent().unwrap().to_str().unwrap().to_string();
}

pub fn quit(s: &String) {
    // quit with a message - don't use this except in main.rs!
    println!("{}", s); 
    process::exit(0x01)
}

pub fn is_executable(path: &Path) -> bool {
    let metadata = match fs::metadata(path) {
        Ok(x) => x, Err(_) => return false,
    };
    let permissions = metadata.permissions();
    if ! metadata.is_file() {
        return false;
    }
    let mode_bits = permissions.mode() & 0o111;
    if mode_bits == 0 {
        return false;
    }
    return true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn test_jet_read_dir_success() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();
        
        // Create some files in the directory
        File::create(path.join("file1.txt")).unwrap();
        File::create(path.join("file2.txt")).unwrap();
        
        let result = jet_read_dir(path);
        assert!(result.is_ok());
        
        let entries: Vec<_> = result.unwrap().collect();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_jet_read_dir_failure() {
        let non_existent = Path::new("/non/existent/directory");
        let result = jet_read_dir(non_existent);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed to read directory"));
    }

    #[test]
    fn test_path_walk_success() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();
        
        // Create some files
        File::create(path.join("file1.txt")).unwrap();
        File::create(path.join("file2.txt")).unwrap();
        File::create(path.join("file3.txt")).unwrap();
        
        let mut count = 0;
        let result = path_walk(path, |_p| {
            count += 1;
            Ok(())
        });
        
        assert!(result.is_ok());
        assert_eq!(count, 3);
    }

    #[test]
    fn test_path_walk_with_error() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();
        
        File::create(path.join("file1.txt")).unwrap();
        File::create(path.join("file2.txt")).unwrap();
        
        let mut count = 0;
        let result = path_walk(path, |_p| {
            count += 1;
            if count == 1 {
                Err("Stop walking".to_string())
            } else {
                Ok(())
            }
        });
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Stop walking");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_jet_file_open_success() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        File::create(&file_path).unwrap();
        
        let result = jet_file_open(&file_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_jet_file_open_failure() {
        let non_existent = Path::new("/non/existent/file.txt");
        let result = jet_file_open(non_existent);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unable to open file"));
    }

    #[test]
    fn test_read_local_file_success() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "Test content").unwrap();
        writeln!(file, "Second line").unwrap();
        
        let result = read_local_file(&file_path);
        assert!(result.is_ok());
        
        let content = result.unwrap();
        assert!(content.contains("Test content"));
        assert!(content.contains("Second line"));
    }

    #[test]
    fn test_read_local_file_failure() {
        let non_existent = Path::new("/non/existent/file.txt");
        let result = read_local_file(non_existent);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unable to open file"));
    }

    #[test]
    fn test_path_basename_as_string() {
        let path = Path::new("/home/user/documents/file.txt");
        let basename = path_basename_as_string(path);
        assert_eq!(basename, "file.txt");
        
        let path2 = Path::new("simple.txt");
        let basename2 = path_basename_as_string(path2);
        assert_eq!(basename2, "simple.txt");
    }

    #[test]
    fn test_path_as_string() {
        let path = Path::new("/home/user/file.txt");
        let path_str = path_as_string(path);
        assert_eq!(path_str, "/home/user/file.txt");
    }

    #[test]
    fn test_directory_as_string() {
        let path = Path::new("/home/user/documents/file.txt");
        let dir = directory_as_string(path);
        assert_eq!(dir, "/home/user/documents");
        
        let path2 = Path::new("/file.txt");
        let dir2 = directory_as_string(path2);
        assert_eq!(dir2, "/");
    }

    #[test]
    fn test_is_executable_regular_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("script.sh");
        let file = File::create(&file_path).unwrap();
        
        // Initially not executable
        assert!(!is_executable(&file_path));
        
        // Make it executable
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&file_path, perms).unwrap();
        
        assert!(is_executable(&file_path));
    }

    #[test]
    fn test_is_executable_non_executable_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("regular.txt");
        let file = File::create(&file_path).unwrap();
        
        // Set permissions to read/write only
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&file_path, perms).unwrap();
        
        assert!(!is_executable(&file_path));
    }

    #[test]
    fn test_is_executable_directory() {
        let temp_dir = TempDir::new().unwrap();
        let dir_path = temp_dir.path().join("subdir");
        fs::create_dir(&dir_path).unwrap();
        
        // Directories should return false even with execute bit
        assert!(!is_executable(&dir_path));
    }

    #[test]
    fn test_is_executable_non_existent() {
        let non_existent = Path::new("/non/existent/file");
        assert!(!is_executable(non_existent));
    }

    // Note: test_quit is omitted because it calls process::exit which
    // interferes with test runners and coverage tools
}