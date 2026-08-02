use gameforge_project_documents::{TaskDraft, load_task_document, render_task_markdown};

#[test]
fn renders_a_draft_that_can_be_loaded_as_a_task_document() {
    let source = render_task_markdown(&TaskDraft {
        id: "TASK-GENERATED".to_owned(),
        title: "Generated task".to_owned(),
        purpose: "Create a generated task document.".to_owned(),
        acceptance_criteria: vec!["AC-GENERATED".to_owned()],
        allowed_paths: vec!["crates/example/**".to_owned()],
        test_paths: vec!["crates/example/tests/**".to_owned()],
        forbidden_paths: vec!["crates/runtime/**".to_owned()],
    });

    let document = load_task_document(&source).expect("generated markdown is valid");
    assert_eq!(document.id().as_str(), "TASK-GENERATED");
    assert_eq!(document.title(), "Generated task");
    assert_eq!(document.acceptance_criteria(), &["AC-GENERATED"]);
}
