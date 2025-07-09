use jetpack::tasks::fields::Field;
use std::collections::HashSet;

#[test]
fn test_field_equality() {
    assert_eq!(Field::Branch, Field::Branch);
    assert_ne!(Field::Branch, Field::Content);
    assert_ne!(Field::Owner, Field::Group);
}

#[test]
fn test_field_clone() {
    let field1 = Field::Mode;
    let field2 = field1.clone();
    assert_eq!(field1, field2);
}

#[test]
fn test_field_debug() {
    let field = Field::Version;
    let debug_str = format!("{:?}", field);
    assert_eq!(debug_str, "Version");
}

#[test]
fn test_all_file_attributes() {
    let attrs = Field::all_file_attributes();
    assert_eq!(attrs.len(), 3);
    assert!(attrs.contains(&Field::Owner));
    assert!(attrs.contains(&Field::Group));
    assert!(attrs.contains(&Field::Mode));
    
    // Verify order
    assert_eq!(attrs[0], Field::Owner);
    assert_eq!(attrs[1], Field::Group);
    assert_eq!(attrs[2], Field::Mode);
}

#[test]
fn test_field_hash() {
    let mut set = HashSet::new();
    set.insert(Field::Branch);
    set.insert(Field::Content);
    set.insert(Field::Branch); // Duplicate should not increase size
    
    assert_eq!(set.len(), 2);
    assert!(set.contains(&Field::Branch));
    assert!(set.contains(&Field::Content));
}

#[test]
fn test_all_field_variants() {
    // Test that we can create each variant
    let fields = vec![
        Field::Branch,
        Field::Content,
        Field::Disable,
        Field::Enable,
        Field::Gecos,
        Field::Gid,
        Field::Group,
        Field::Groups,
        Field::Mode,
        Field::Owner,
        Field::Restart,
        Field::Shell,
        Field::Start,
        Field::Stop,
        Field::Uid,
        Field::Users,
        Field::Version,
    ];
    
    // Ensure all are unique
    let unique_fields: HashSet<_> = fields.iter().collect();
    assert_eq!(unique_fields.len(), fields.len());
}