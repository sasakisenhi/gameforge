use gameforge_domain::{
    AcceptanceDecision, AcceptanceResultId, Actor, ArtifactHash, ArtifactUri, Build, BuildId,
    Candidate, CandidateId, CandidateRevision, CommitSha, ContractRevision, DomainError,
    EvidenceKey, EvidenceKind, HealthFlag, IncludedTaskRun, MergeGate, PlaytestSession,
    PlaytestSessionId, RecordedAt, Task, TaskCommand, TaskDependency, TaskDependencyKind, TaskId,
    TaskRun, TaskRunCommand, TaskRunId, TaskRunStatus, decide_task, decide_task_run, evolve_task,
    evolve_task_run, validate_task_graph,
};

fn commit(value: char) -> CommitSha {
    CommitSha::new(value.to_string().repeat(40)).expect("valid commit")
}

fn succeeded_run() -> TaskRun {
    let mut run = TaskRun::new(
        TaskRunId::new("RUN-1").unwrap(),
        TaskId::new("TASK-1").unwrap(),
        ContractRevision::new(1).unwrap(),
        commit('a'),
    );
    for command in [
        TaskRunCommand::Prepare,
        TaskRunCommand::StartAgent {
            has_resource_lease: true,
            has_worktree_lease: true,
        },
        TaskRunCommand::StartLocalChecks,
        TaskRunCommand::CompleteLocalChecks {
            required_checks_passed: true,
            scope_check_passed: true,
            final_suite_passed: true,
            behavior_changed: true,
            red_evidence_present: true,
            green_evidence_present: true,
            head_commit: commit('b'),
        },
    ] {
        let events = decide_task_run(&run, command).unwrap();
        run = events.iter().fold(run, evolve_task_run);
    }
    run
}

#[test]
fn task_id_and_commit_are_validated() {
    assert!(TaskId::new("TASK-1").is_ok());
    assert_eq!(TaskId::new("  "), Err(DomainError::EmptyValue("TaskId")));
    assert!(CommitSha::new("abc").is_err());
    assert!(CommitSha::new("z".repeat(40)).is_err());
}

#[test]
fn task_becomes_ready_only_through_a_valid_contract_revision() {
    let task = Task::draft(TaskId::new("TASK-1").unwrap());
    let events = decide_task(
        &task,
        TaskCommand::MarkReady {
            contract_revision: ContractRevision::new(1).unwrap(),
        },
    )
    .unwrap();
    let ready = events.iter().fold(task, evolve_task);

    assert!(ready.is_ready());
    assert_eq!(
        ready.contract_revision(),
        Some(ContractRevision::new(1).unwrap())
    );
    assert!(matches!(
        decide_task(&ready, TaskCommand::MarkAcceptedAfterMerge),
        Err(DomainError::InvalidTransition { .. })
    ));
}

#[test]
fn dependency_graph_rejects_missing_self_and_cycles() {
    let one = TaskId::new("TASK-1").unwrap();
    let two = TaskId::new("TASK-2").unwrap();
    let missing = TaskId::new("TASK-X").unwrap();

    assert!(matches!(
        validate_task_graph(
            std::slice::from_ref(&one),
            &[TaskDependency::new(
                one.clone(),
                missing,
                TaskDependencyKind::BlocksStart,
                "APIが必要",
            )
            .unwrap()],
        ),
        Err(DomainError::MissingDependency { .. })
    ));
    assert!(matches!(
        TaskDependency::new(
            one.clone(),
            one.clone(),
            TaskDependencyKind::BlocksStart,
            "自己依存",
        ),
        Err(DomainError::SelfDependency(_))
    ));
    assert!(matches!(
        validate_task_graph(
            &[one.clone(), two.clone()],
            &[
                TaskDependency::new(
                    one.clone(),
                    two.clone(),
                    TaskDependencyKind::BlocksStart,
                    "先行",
                )
                .unwrap(),
                TaskDependency::new(two, one, TaskDependencyKind::BlocksIntegration, "逆向き",)
                    .unwrap(),
            ],
        ),
        Err(DomainError::DependencyCycle(_))
    ));
}

#[test]
fn dependency_order_is_stable_and_places_prerequisites_first() {
    let one = TaskId::new("TASK-1").unwrap();
    let two = TaskId::new("TASK-2").unwrap();
    let three = TaskId::new("TASK-3").unwrap();
    let dependencies = [
        TaskDependency::new(
            three.clone(),
            one.clone(),
            TaskDependencyKind::BlocksStart,
            "基盤",
        )
        .unwrap(),
        TaskDependency::new(
            three.clone(),
            two.clone(),
            TaskDependencyKind::BlocksIntegration,
            "設定",
        )
        .unwrap(),
    ];

    let order =
        validate_task_graph(&[three.clone(), two.clone(), one.clone()], &dependencies).unwrap();
    assert_eq!(order, vec![one, two, three]);
}

#[test]
fn task_run_requires_leases_and_tdd_evidence() {
    let run = TaskRun::new(
        TaskRunId::new("RUN-1").unwrap(),
        TaskId::new("TASK-1").unwrap(),
        ContractRevision::new(1).unwrap(),
        commit('a'),
    );
    let events = decide_task_run(&run, TaskRunCommand::Prepare).unwrap();
    let prepared = events.iter().fold(run, evolve_task_run);

    assert!(matches!(
        decide_task_run(
            &prepared,
            TaskRunCommand::StartAgent {
                has_resource_lease: false,
                has_worktree_lease: true,
            },
        ),
        Err(DomainError::MissingLease)
    ));

    let started = decide_task_run(
        &prepared,
        TaskRunCommand::StartAgent {
            has_resource_lease: true,
            has_worktree_lease: true,
        },
    )
    .unwrap()
    .iter()
    .fold(prepared, evolve_task_run);
    let checking = decide_task_run(&started, TaskRunCommand::StartLocalChecks)
        .unwrap()
        .iter()
        .fold(started, evolve_task_run);
    assert!(matches!(
        decide_task_run(
            &checking,
            TaskRunCommand::CompleteLocalChecks {
                required_checks_passed: true,
                scope_check_passed: true,
                final_suite_passed: true,
                behavior_changed: true,
                red_evidence_present: false,
                green_evidence_present: true,
                head_commit: commit('b'),
            },
        ),
        Err(DomainError::MissingTddEvidence)
    ));
}

#[test]
fn blocking_health_flags_prevent_task_run_success() {
    let mut run = succeeded_run();
    assert_eq!(run.status(), TaskRunStatus::Succeeded);

    run = run.with_health_flag(HealthFlag::ScopeViolation);
    assert!(!run.is_integrable());
}

#[test]
fn build_and_acceptance_are_bound_to_one_candidate_commit() {
    let run = succeeded_run();
    let included = IncludedTaskRun::from_succeeded(&run).unwrap();
    let candidate = Candidate::technically_verified(
        CandidateId::new("CAND-1").unwrap(),
        CandidateRevision::new(1).unwrap(),
        commit('b'),
        vec![included],
    )
    .unwrap();
    let mut build = Build::request(BuildId::new("BUILD-1").unwrap(), &candidate).unwrap();
    build.start().unwrap();
    assert!(matches!(
        build.complete(
            commit('c'),
            ArtifactUri::new("file:///tmp/game").unwrap(),
            ArtifactHash::new("sha256:abcd").unwrap(),
        ),
        Err(DomainError::CommitMismatch { .. })
    ));
    build
        .complete(
            commit('b'),
            ArtifactUri::new("file:///tmp/game").unwrap(),
            ArtifactHash::new("sha256:abcd").unwrap(),
        )
        .unwrap();

    let mut session =
        PlaytestSession::start(PlaytestSessionId::new("PLAY-1").unwrap(), &build).unwrap();
    session.begin().unwrap();
    let result = session
        .record_result(
            &build,
            AcceptanceResultId::new("RESULT-1").unwrap(),
            AcceptanceDecision::Accepted,
            "砂の挙動を確認した",
            Actor::new("human:sasaki").unwrap(),
            RecordedAt::new("2026-08-01T12:00:00+09:00").unwrap(),
        )
        .unwrap();

    assert!(result.applies_to(&candidate, &build));
}

#[test]
fn evidence_and_acceptance_do_not_apply_after_revision_changes() {
    let run = succeeded_run();
    let included = IncludedTaskRun::from_succeeded(&run).unwrap();
    let candidate = Candidate::technically_verified(
        CandidateId::new("CAND-1").unwrap(),
        CandidateRevision::new(2).unwrap(),
        commit('c'),
        vec![included],
    )
    .unwrap();
    let old_evidence = EvidenceKey::new(
        CandidateId::new("CAND-1").unwrap(),
        CandidateRevision::new(1).unwrap(),
        commit('b'),
        EvidenceKind::ContinuousIntegration,
        "ATTEMPT-1",
    )
    .unwrap();

    assert!(!old_evidence.is_current_for(&candidate));
}

#[test]
fn merge_gate_rejects_any_commit_mismatch() {
    let run = succeeded_run();
    let included = IncludedTaskRun::from_succeeded(&run).unwrap();
    let mut candidate = Candidate::technically_verified(
        CandidateId::new("CAND-1").unwrap(),
        CandidateRevision::new(1).unwrap(),
        commit('b'),
        vec![included],
    )
    .unwrap();
    let mut build = Build::request(BuildId::new("BUILD-1").unwrap(), &candidate).unwrap();
    build.start().unwrap();
    build
        .complete(
            commit('b'),
            ArtifactUri::new("file:///tmp/game").unwrap(),
            ArtifactHash::new("sha256:abcd").unwrap(),
        )
        .unwrap();
    let mut session =
        PlaytestSession::start(PlaytestSessionId::new("PLAY-1").unwrap(), &build).unwrap();
    session.begin().unwrap();
    let result = session
        .record_result(
            &build,
            AcceptanceResultId::new("RESULT-1").unwrap(),
            AcceptanceDecision::Accepted,
            "受け入れ",
            Actor::new("human:sasaki").unwrap(),
            RecordedAt::new("2026-08-01T12:00:00+09:00").unwrap(),
        )
        .unwrap();
    candidate.apply_acceptance(&build, &result).unwrap();

    let gate = MergeGate {
        candidate: &candidate,
        build: &build,
        acceptance: &result,
        pull_request_head: commit('c'),
        required_ci_commit: commit('b'),
        required_ci_passed: true,
        ai_review_commit: commit('b'),
        ai_review_passed: true,
        main_base_is_compatible: true,
    };
    assert!(matches!(
        gate.evaluate(),
        Err(DomainError::MergeGateRejected(_))
    ));
}
