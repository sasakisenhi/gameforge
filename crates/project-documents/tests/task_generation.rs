use gameforge_project_documents::{
    TaskDocumentStatus, TaskDraft, load_task_document, mock_task_draft, promote_task_to_ready,
    render_task_markdown,
};

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

#[test]
fn renders_empty_test_paths_without_merging_the_next_front_matter_field() {
    let source = render_task_markdown(&mock_task_draft("TASK-EMPTY", "Add a task"));

    assert!(source.contains("test_paths: []\nforbidden_paths:"));
    load_task_document(&source).expect("empty test_paths must remain valid YAML");
}

#[test]
fn promotes_a_draft_to_ready_without_changing_the_contract() {
    let source = render_task_markdown(&mock_task_draft("TASK-READY", "Add gravity"));
    let ready = promote_task_to_ready(&source).expect("draft can be approved");
    let document = load_task_document(&ready).expect("approved document remains valid");

    assert_eq!(document.status(), TaskDocumentStatus::Ready);
    assert!(ready.contains("status: ready"));
    assert!(ready.contains("# 目的\n\nAdd gravity"));
}

#[test]
fn refuses_to_promote_an_already_ready_task() {
    let source = render_task_markdown(&mock_task_draft("TASK-READY", "Add gravity")).replacen(
        "status: draft",
        "status: ready",
        1,
    );

    assert!(promote_task_to_ready(&source).is_err());
}

#[test]
fn mock_conversation_produces_a_previewable_draft() {
    let draft = mock_task_draft("TASK-MOCK", "Add a sparkle effect\nMake it configurable.");

    assert_eq!(draft.title, "Add a sparkle effect");
    assert_eq!(draft.purpose, "Add a sparkle effect\nMake it configurable.");
    assert!(render_task_markdown(&draft).contains("status: draft"));
}
