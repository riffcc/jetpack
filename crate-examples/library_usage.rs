// Example of using JetPack as a library

use jetpack::{JetpackConfig, PlaybookRunner, TerminalOutputHandler, OutputHandler, LogLevel, RecapData};
use jetpack::output::NullOutputHandler;
use std::sync::Arc;

fn main() -> jetpack::Result<()> {
    // Example 1: Simple playbook run with default settings
    simple_example()?;
    
    // Example 2: Advanced configuration
    advanced_example()?;
    
    // Example 3: Custom output handler
    custom_output_example()?;
    
    // Example 4: Using the builder API
    builder_example()?;
    
    Ok(())
}

fn simple_example() -> jetpack::Result<()> {
    println!("=== Simple Example ===");
    
    // Create configuration
    let config = JetpackConfig::new()
        .playbook("./playbook.yml")
        .inventory("./inventory/hosts.yml")
        .ssh();
    
    // Create runner with terminal output
    let output = Arc::new(TerminalOutputHandler::new(1));
    let runner = PlaybookRunner::new(config)
        .with_output_handler(output);
    
    // Run the playbook
    let result = runner.run()?;
    println!("Playbook completed. Success: {}", result.success);
    
    Ok(())
}

fn advanced_example() -> jetpack::Result<()> {
    println!("\n=== Advanced Example ===");
    
    // Create configuration with many options
    let config = JetpackConfig::new()
        .playbook("./site.yml")
        .inventory("./production/inventory")
        .ssh()
        .user("deploy".to_string())
        .sudo("root".to_string())
        .port(2222)
        .threads(10)
        .limit_hosts(vec!["web1".to_string(), "web2".to_string()])
        .tags(vec!["deploy".to_string(), "restart".to_string()])
        .check_mode(true)
        .forward_agent(true);
    
    // Run with minimal output
    let runner = PlaybookRunner::new(config);
    let result = runner.run()?;
    
    println!("Check mode run completed. Hosts processed: {}", result.hosts_processed);
    
    Ok(())
}

fn custom_output_example() -> jetpack::Result<()> {
    println!("\n=== Custom Output Handler Example ===");
    
    // Define a custom output handler
    struct JsonOutputHandler;
    
    impl OutputHandler for JsonOutputHandler {
        fn on_playbook_start(&self, playbook_path: &str) {
            println!(r#"{{"event": "playbook_start", "path": "{}"}}"#, playbook_path);
        }
        
        fn on_playbook_end(&self, playbook_path: &str, success: bool) {
            println!(r#"{{"event": "playbook_end", "path": "{}", "success": {}}}"#, 
                playbook_path, success);
        }
        
        fn on_play_start(&self, play_name: &str, hosts: Vec<String>) {
            println!(r#"{{"event": "play_start", "name": "{}", "hosts": {:?}}}"#, 
                play_name, hosts);
        }
        
        fn on_play_end(&self, play_name: &str) {
            println!(r#"{{"event": "play_end", "name": "{}"}}"#, play_name);
        }
        
        fn on_task_start(&self, task_name: &str, host_count: usize) {
            println!(r#"{{"event": "task_start", "name": "{}", "host_count": {}}}"#, 
                task_name, host_count);
        }
        
        fn on_task_host_result(&self, host: &jetpack::inventory::hosts::Host, 
                              _task: &jetpack::tasks::request::TaskRequest, 
                              response: &jetpack::tasks::response::TaskResponse) {
            let status = if !response.is_ok() {
                "failed"
            } else if response.is_changed() {
                "changed"
            } else {
                "ok"
            };
            
            println!(r#"{{"event": "task_result", "host": "{}", "status": "{}"}}"#, 
                host.name, status);
        }
        
        fn on_task_end(&self, task_name: &str) {
            println!(r#"{{"event": "task_end", "name": "{}"}}"#, task_name);
        }
        
        fn on_handler_start(&self, handler_name: &str) {
            println!(r#"{{"event": "handler_start", "name": "{}"}}"#, handler_name);
        }
        
        fn on_handler_end(&self, handler_name: &str) {
            println!(r#"{{"event": "handler_end", "name": "{}"}}"#, handler_name);
        }
        
        fn on_recap(&self, recap: RecapData) {
            println!(r#"{{"event": "recap", "host": "{}", "ok": {}, "changed": {}, "failed": {}}}"#,
                recap.host, recap.ok, recap.changed, recap.failed);
        }
        
        fn log(&self, level: LogLevel, message: &str) {
            let level_str = match level {
                LogLevel::Debug => "debug",
                LogLevel::Info => "info",
                LogLevel::Warning => "warning",
                LogLevel::Error => "error",
            };
            println!(r#"{{"event": "log", "level": "{}", "message": "{}"}}"#, 
                level_str, message);
        }
    }
    
    // Use the custom handler
    let config = JetpackConfig::new()
        .playbook("./test.yml")
        .local();
    
    let runner = PlaybookRunner::new(config)
        .with_output_handler(Arc::new(JsonOutputHandler));
    
    runner.run()?;
    
    Ok(())
}

fn builder_example() -> jetpack::Result<()> {
    println!("\n=== Builder API Example ===");
    
    // Use the convenient builder API
    let result = jetpack::run_playbook("./deploy.yml")
        .inventory("./staging/hosts")
        .ssh()
        .user("admin")
        .limit_hosts(vec!["app-server-1".to_string()])
        .threads(5)
        .run()?;
    
    println!("Deployment completed! Hosts processed: {}", result.hosts_processed);
    
    Ok(())
}