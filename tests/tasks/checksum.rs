use jetpack::tasks::checksum::*;

#[test]
fn test_sha512_empty_string() {
    let result = sha512(&"".to_string());
    // SHA-512 of empty string
    assert_eq!(result, "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e");
}

#[test]
fn test_sha512_hello_world() {
    let result = sha512(&"Hello World".to_string());
    // SHA-512 of "Hello World"
    assert_eq!(result, "2c74fd17edafd80e8447b0d46741ee243b7eb74dd2149a0ab1b9246fb30382f27e853d8585719e0e67cbda0daa8f51671064615d645ae27acb15bfb1447f459b");
}

#[test]
fn test_sha512_with_newline() {
    let result = sha512(&"test\n".to_string());
    assert_eq!(result.len(), 128); // SHA-512 produces 128 hex characters
}

#[test]
fn test_sha512_unicode() {
    let result = sha512(&"Hello 世界".to_string());
    assert_eq!(result.len(), 128);
    // Different from ASCII-only string
    assert_ne!(result, sha512(&"Hello World".to_string()));
}

#[test]
fn test_sha512_deterministic() {
    let input = "Deterministic test".to_string();
    let result1 = sha512(&input);
    let result2 = sha512(&input);
    assert_eq!(result1, result2);
}