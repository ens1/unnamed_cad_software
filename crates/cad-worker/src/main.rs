use cad_core::{Result, protocol::*};
use cad_kernel::evaluate::Evaluator;
use std::io;
fn main() {
    if let Err(e) = run() {
        eprintln!("Geometry worker stopped: {e}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut evaluator = Evaluator::default();
    let mut display = cad_kernel::display_mesh::Service::default();
    loop {
        let request = match read_frame::<WorkerRequest>(&mut input) {
            Ok(r) => r,
            Err(cad_core::CadError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        };
        let identity = match &request {
            WorkerRequest::DisplayMesh { request } => Some(request.identity.clone()),
            WorkerRequest::AnalyzeSketch { identity, .. }
            | WorkerRequest::PublishGenerate { identity, .. }
            | WorkerRequest::ReadmePreview { identity, .. }
            | WorkerRequest::SolveSketch3d { identity, .. }
            | WorkerRequest::ManipulateAssembly { identity, .. }
            | WorkerRequest::Evaluate { identity, .. }
            | WorkerRequest::Export { identity, .. }
            | WorkerRequest::Release { identity, .. }
            | WorkerRequest::Import { identity, .. } => Some(identity.clone()),
            WorkerRequest::Shutdown => break,
        };
        // Reject incompatible application/worker pairs before any handler can
        // change files or turn a version mismatch into a sketch-source error.
        // Keep the caller's identity so the desktop can attach this diagnostic
        // to the current draft without accepting an incompatible evaluation.
        if let Some(request_identity) = &identity
            && (request_identity.protocol != PROTOCOL_VERSION
                || request_identity.backend != evaluator.fingerprint())
        {
            write_frame(
                &mut output,
                &WorkerResponse::Error {
                    identity: identity.clone(),
                    message: format!(
                        "TCAD and its geometry worker use different versions. Restart TCAD to load the installed update.\nIf this continues, reinstall TCAD.\nRequested protocol {}, backend {}; running protocol {}, backend {}.",
                        request_identity.protocol,
                        request_identity.backend,
                        PROTOCOL_VERSION,
                        evaluator.fingerprint()
                    ),
                },
            )?;
            continue;
        }
        let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match request {
            WorkerRequest::PublishGenerate {
                identity,
                document,
                project_root,
                staging_directory,
                preferences,
                display_tolerance_mm,
            } => cad_core::publication::generate(
                &mut evaluator,
                &identity,
                &document,
                std::path::Path::new(&project_root),
                std::path::Path::new(&staging_directory),
                &preferences,
                display_tolerance_mm,
            )
            .map(|package| WorkerResponse::PublishGenerated {
                identity,
                package: Box::new(package),
            }),
            WorkerRequest::ReadmePreview {
                identity,
                document,
                project_root,
                display_tolerance_mm,
            } => {
                let evaluation = evaluator.evaluate(
                    identity.clone(),
                    &document,
                    std::path::Path::new(&project_root),
                    display_tolerance_mm,
                )?;
                let preview = cad_core::readme_preview::build(&document, &evaluation)?;
                Ok(WorkerResponse::ReadmePreview {
                    identity,
                    preview: Box::new(preview),
                })
            }
            WorkerRequest::DisplayMesh { request } => display
                .mesh(*request, evaluator.fingerprint())
                .map(|output| WorkerResponse::DisplayMesh {
                    output: Box::new(output),
                }),
            WorkerRequest::AnalyzeSketch {
                identity,
                document,
                owner,
                sketch,
            } => {
                use cad_core::solver::ConstraintSolver;
                if identity.protocol != PROTOCOL_VERSION
                    || identity.backend != evaluator.fingerprint()
                    || identity.source != document.fingerprint()
                    || document
                        .parts
                        .get(&owner)
                        .is_none_or(|part| !part.sketches.contains_key(&sketch.id))
                {
                    return Err(cad_core::CadError::Invalid(
                        "Sketch analysis source identity mismatch".into(),
                    ));
                }
                cad_core::solver::NativeSolver
                    .analyze_sketch(&document, &owner, &sketch)
                    .map(|analysis| WorkerResponse::SketchAnalyzed {
                        identity,
                        owner,
                        sketch: sketch.id,
                        analysis: Box::new(analysis),
                    })
            }
            WorkerRequest::SolveSketch3d {
                identity,
                document,
                feature,
                editor,
                sequence,
                targets,
            } => {
                use cad_core::solver::ConstraintSolver;
                let solved = (|| -> Result<_> {
                    if identity.protocol != PROTOCOL_VERSION
                        || identity.backend != evaluator.fingerprint()
                        || identity.source != document.fingerprint()
                    {
                        return Err(cad_core::CadError::Invalid(
                            "3D sketch goal source identity mismatch".into(),
                        ));
                    }
                    document.validate()?;
                    let configured = cad_core::configuration::effective(&document)?;
                    let source = configured
                        .parts
                        .get(&feature.definition)
                        .and_then(|part| {
                            part.features
                                .iter()
                                .find(|source| source.id == feature.node)
                        })
                        .filter(|source| !source.suppressed)
                        .ok_or_else(|| {
                            cad_core::CadError::Invalid(
                                "3D sketch goal feature is missing or suppressed".into(),
                            )
                        })?;
                    let cad_core::Operation::Sketch3d { definition } = &source.operation else {
                        return Err(cad_core::CadError::Invalid(
                            "Cursor goals require a 3D sketch".into(),
                        ));
                    };
                    let mut solved = (**definition).clone();
                    let report = if targets.is_empty() {
                        cad_core::solver::NativeSolver.solve_sketch3d(
                            &configured,
                            &feature.definition,
                            &mut solved,
                        )?
                    } else {
                        cad_core::solver::NativeSolver.drag_sketch3d(
                            &configured,
                            &feature.definition,
                            &mut solved,
                            &targets,
                        )?
                    };
                    Ok((solved, report))
                })();
                Ok(match solved {
                    Ok((solved, report)) => WorkerResponse::Sketch3dSolved {
                        identity,
                        feature,
                        editor,
                        sequence,
                        solved: Box::new(solved),
                        report,
                    },
                    Err(error) => WorkerResponse::Sketch3dSolveFailed {
                        identity,
                        feature,
                        editor,
                        sequence,
                        message: error.to_string(),
                    },
                })
            }
            WorkerRequest::ManipulateAssembly {
                identity,
                document,
                assemblies,
                intent,
            } => {
                if identity.protocol != PROTOCOL_VERSION
                    || identity.backend != evaluator.fingerprint()
                    || identity.source != document.fingerprint()
                {
                    return Err(cad_core::CadError::Invalid(
                        "Assembly movement source identity mismatch".into(),
                    ));
                }
                cad_core::assembly_motion::solve(&document, &assemblies, &intent).map(|outcome| {
                    WorkerResponse::AssemblyManipulated {
                        identity,
                        outcome: Box::new(outcome),
                    }
                })
            }
            WorkerRequest::Evaluate {
                identity,
                document,
                project_root,
                display_tolerance_mm,
                operation_preview,
            } => evaluator
                .evaluate_with_operation_preview(
                    identity,
                    &document,
                    std::path::Path::new(&project_root),
                    display_tolerance_mm,
                    operation_preview,
                )
                .map(|result| WorkerResponse::Evaluation { result }),
            WorkerRequest::Export {
                identity,
                document,
                project_root,
                path,
                format,
                display_tolerance_mm,
            } => evaluator
                .export(
                    &document,
                    std::path::Path::new(&project_root),
                    std::path::Path::new(&path),
                    &format,
                    display_tolerance_mm,
                )
                .map(|()| WorkerResponse::Exported { identity, path }),
            WorkerRequest::Release {
                identity,
                document,
                project_root,
                path,
                display_tolerance_mm,
            } => cad_core::release::create(
                &mut evaluator,
                &document,
                std::path::Path::new(&project_root),
                std::path::Path::new(&path),
                display_tolerance_mm,
            )
            .map(|_| WorkerResponse::Exported { identity, path }),
            WorkerRequest::Import {
                identity,
                mut document,
                project_root,
                resource,
            } => {
                let imported = cad_kernel::exchange::import_structure(
                    &mut document,
                    &resource,
                    std::path::Path::new(&project_root),
                )?;
                let key = cad_core::id();
                let name = cad_core::presentation::definition_name(&document, &imported);
                document
                    .assemblies
                    .get_mut(&document.root)
                    .ok_or_else(|| {
                        cad_core::CadError::Invalid("Import needs an assembly root".into())
                    })?
                    .instances
                    .insert(
                        key.clone(),
                        cad_core::Instance {
                            id: key,
                            name,
                            definition: imported,
                            placement: Default::default(),
                            configuration: None,
                            finish: None,
                            fixed: true,
                            translation_expressions: None,
                        },
                    );
                document.validate()?;
                Ok(WorkerResponse::Imported { identity, document })
            }
            WorkerRequest::Shutdown => unreachable!(),
        }));
        let mut response = match response {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => WorkerResponse::Error {
                identity,
                message: e.to_string(),
            },
            Err(_) => {
                evaluator = Evaluator::default();
                display = cad_kernel::display_mesh::Service::default();
                WorkerResponse::Error {
                    identity,
                    message: "Geometry kernel panicked; worker geometry was invalidated".into(),
                }
            }
        };
        if let Err(error) = write_frame(&mut output, &response) {
            // write_frame checks size before writing. If the optional display
            // cache causes an oversized response, retain the valid CAD result.
            let oversized = matches!(&error, cad_core::CadError::Invalid(message) if message == "IPC frame exceeds limit");
            let WorkerResponse::Evaluation { result } = &mut response else {
                return Err(error);
            };
            let Some(preview) = result.operation_preview.as_mut().filter(|_| oversized) else {
                return Err(error);
            };
            preview.changes.clear();
            preview.baseline_instances.clear();
            preview.diagnostics = vec![Diagnostic {
                severity: "warning".into(),
                target: preview.focus.as_ref().map(|focus| focus.node.clone()),
                message: "Localized highlights were omitted to keep the validated CAD result within the worker transport limit".into(),
            }];
            write_frame(&mut output, &response)?;
        }
    }
    Ok(())
}
