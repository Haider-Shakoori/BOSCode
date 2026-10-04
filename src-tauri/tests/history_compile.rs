#[allow(dead_code)]
#[path = "../src/history.rs"]
mod history;

#[test]
fn history_module_is_linked_into_regression_tests() {
    let state = history::HistoryState::default();
    drop(state);
}
