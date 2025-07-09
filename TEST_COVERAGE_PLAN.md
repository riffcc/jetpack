# JetPack Test Coverage Plan

## Current Status
- **Current Coverage**: 9.68% (468/4837 lines)
- **Target Coverage**: 80% (3870 lines)
- **Lines to Cover**: ~3402 lines

## Completed Test Modules

### ✅ Utility Modules
1. **src/util/yaml.rs** - Full test coverage
   - `blend_variables` function with all edge cases
   - `show_yaml_error_in_context` function

2. **src/util/io.rs** - Full test coverage
   - File operations (`jet_read_dir`, `jet_file_open`, `read_local_file`)
   - Path utilities (`path_walk`, `path_basename_as_string`, etc.)
   - `is_executable` function
   - Note: `quit` function excluded due to process::exit

3. **src/util/terminal.rs** - Full test coverage
   - `markdown_print`, `banner`, `two_column_table`, `captioned_display`

### ✅ Inventory System
1. **src/inventory/hosts.rs** - Comprehensive tests
   - Host creation and OS detection
   - Group management
   - Variable handling and blending
   - Notification system
   - Checksum cache

2. **src/inventory/groups.rs** - Comprehensive tests
   - Group hierarchy (parents/children)
   - Host management
   - Variable inheritance and blending
   - Ancestor/descendant traversal

### ✅ CLI Parser (Partial)
1. **src/cli/parser.rs** - Basic tests
   - CLI mode validation
   - Argument mapping
   - Parser initialization
   - Environment variable handling

## Modules Requiring Tests

### 🔲 High Priority (Core Functionality)
1. **src/tasks/** - Task execution engine
   - `common.rs` - Task trait and interfaces
   - `logic.rs` - Conditional logic evaluation
   - `request.rs` - Task request handling
   - `response.rs` - Task response handling
   - `fields.rs` - Field validation

2. **src/playbooks/** - Playbook processing
   - `traversal.rs` - Playbook traversal logic
   - `visitor.rs` - Visitor pattern implementation
   - `context.rs` - Execution context
   - `task_fsm.rs` - Task state machine

3. **src/connection/** - Connection management
   - `connection.rs` - Base connection trait
   - `local.rs` - Local execution
   - `ssh.rs` - SSH connections
   - `factory.rs` - Connection factory

### 🔲 Medium Priority (Module System)
1. **src/modules/** - Individual modules
   - Focus on testing the module trait implementation
   - Test a few representative modules from each category:
     - `files/` - File operations
     - `packages/` - Package management
     - `services/` - Service management
     - `commands/` - Command execution

2. **src/handle/** - Handle system
   - `handle.rs` - Base handle functionality
   - `local.rs` - Local handle
   - `remote.rs` - Remote handle
   - `template.rs` - Template processing

### 🔲 Lower Priority (Supporting Code)
1. **src/registry/** - Module registry
2. **src/cli/show.rs** - Inventory display
3. **src/cli/playbooks.rs** - Playbook execution modes

## Testing Strategy

### Quick Wins for Coverage
1. **Focus on pure functions** - These are easiest to test
2. **Test error paths** - Often missed but add significant coverage
3. **Test constructors and simple getters/setters**
4. **Mock external dependencies** (filesystem, network)

### Test Patterns to Use
```rust
// For modules with external dependencies
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    // Helper to create test fixtures
    fn setup_test_env() -> TempDir {
        let temp = TempDir::new().unwrap();
        // Setup test files/directories
        temp
    }
}

// For testing traits
#[cfg(test)]
mod tests {
    use super::*;
    
    struct MockImplementation {
        // fields for test control
    }
    
    impl TraitName for MockImplementation {
        // minimal implementation for testing
    }
}
```

## Next Steps to Reach 80% Coverage

1. **Add tests for tasks module** (~500 lines)
   - This is core functionality and will give good coverage
   
2. **Add tests for playbook processing** (~800 lines)
   - Critical path for the application
   
3. **Add basic connection tests** (~400 lines)
   - Mock the SSH/network parts
   
4. **Add module trait tests** (~300 lines per category)
   - Pick 2-3 modules from each category
   
5. **Add handle system tests** (~300 lines)

This plan would add approximately 3000+ lines of coverage, bringing the total to ~3500 lines covered, which would be about 72% coverage. Additional strategic tests in the remaining modules would push past the 80% target.

## Tips for Efficient Test Writing

1. Use test generators/macros for similar test patterns
2. Focus on public API first
3. Use property-based testing for complex logic
4. Mock external systems (SSH, filesystem where appropriate)
5. Test both success and error paths
6. Use test fixtures to reduce boilerplate