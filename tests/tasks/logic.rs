use jetpack::tasks::logic::*;

#[test]
fn test_empty_items_vector() {
    let empty = empty_items_vector();
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0], serde_yaml::Value::Bool(true));
}

#[test]
fn test_items_input_enum() {
    // Test ItemsString variant
    let items_str = ItemsInput::ItemsString("my_var".to_string());
    match items_str {
        ItemsInput::ItemsString(s) => assert_eq!(s, "my_var"),
        _ => panic!("Expected ItemsString"),
    }
    
    // Test ItemsList variant
    let items_list = ItemsInput::ItemsList(vec!["a".to_string(), "b".to_string()]);
    match items_list {
        ItemsInput::ItemsList(l) => assert_eq!(l.len(), 2),
        _ => panic!("Expected ItemsList"),
    }
}

#[test]
fn test_pre_logic_input_struct() {
    let pre_logic = PreLogicInput {
        condition: Some("test_condition".to_string()),
        subscribe: Some("test_event".to_string()), 
        sudo: Some("root".to_string()),
        items: Some(ItemsInput::ItemsList(vec!["item1".to_string()])),
        tags: Some(vec!["tag1".to_string()]),
        delegate_to: Some("host1".to_string()),
    };
    
    assert_eq!(pre_logic.condition, Some("test_condition".to_string()));
    assert_eq!(pre_logic.subscribe, Some("test_event".to_string()));
    assert_eq!(pre_logic.sudo, Some("root".to_string()));
    assert!(matches!(pre_logic.items, Some(ItemsInput::ItemsList(_))));
    assert_eq!(pre_logic.tags, Some(vec!["tag1".to_string()]));
    assert_eq!(pre_logic.delegate_to, Some("host1".to_string()));
}

#[test]
fn test_post_logic_input_struct() {
    let post_logic = PostLogicInput {
        notify: Some("handler".to_string()),
        ignore_errors: Some("true".to_string()),
        retry: Some("3".to_string()),
        delay: Some("5".to_string()),
    };
    
    assert_eq!(post_logic.notify, Some("handler".to_string()));
    assert_eq!(post_logic.ignore_errors, Some("true".to_string()));
    assert_eq!(post_logic.retry, Some("3".to_string()));
    assert_eq!(post_logic.delay, Some("5".to_string()));
}

#[test]
fn test_pre_logic_evaluated_struct() {
    let evaluated = PreLogicEvaluated {
        condition: Some("evaluated_condition".to_string()),
        subscribe: Some("evaluated_event".to_string()),
        sudo: Some("evaluated_user".to_string()),
        items: Some(ItemsInput::ItemsString("items_var".to_string())),
        tags: Some(vec!["tag1".to_string(), "tag2".to_string()]),
    };
    
    assert_eq!(evaluated.condition, Some("evaluated_condition".to_string()));
    assert_eq!(evaluated.subscribe, Some("evaluated_event".to_string()));
    assert_eq!(evaluated.sudo, Some("evaluated_user".to_string()));
    assert!(matches!(evaluated.items, Some(ItemsInput::ItemsString(_))));
    assert_eq!(evaluated.tags, Some(vec!["tag1".to_string(), "tag2".to_string()]));
}

#[test]
fn test_post_logic_evaluated_struct() {
    let evaluated = PostLogicEvaluated {
        notify: Some("handler_name".to_string()),
        ignore_errors: true,
        retry: 3,
        delay: 5,
    };
    
    assert_eq!(evaluated.notify, Some("handler_name".to_string()));
    assert_eq!(evaluated.ignore_errors, true);
    assert_eq!(evaluated.retry, 3);
    assert_eq!(evaluated.delay, 5);
}

#[test]
fn test_post_logic_evaluated_defaults() {
    let evaluated = PostLogicEvaluated {
        notify: None,
        ignore_errors: false,
        retry: 0,
        delay: 1,
    };
    
    assert_eq!(evaluated.notify, None);
    assert_eq!(evaluated.ignore_errors, false);
    assert_eq!(evaluated.retry, 0);
    assert_eq!(evaluated.delay, 1);
}