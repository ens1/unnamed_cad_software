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
    loop {
        let request = match read_frame::<WorkerRequest>(&mut input) {
            Ok(r) => r,
            Err(cad_core::CadError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        };
        let identity = match &request {
            WorkerRequest::AnalyzeSketch { identity, .. }
            | WorkerRequest::ManipulateAssembly { identity, .. }
            | WorkerRequest::Evaluate { identity, .. }
            | WorkerRequest::Export { identity, .. }
            | WorkerRequest::Release { identity, .. }
            | WorkerRequest::Import { identity, .. } => Some(identity.clone()),
            WorkerRequest::Shutdown => break,
        };
        let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match request {
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
                tolerance,
                operation_preview,
            } => evaluator
                .evaluate_with_operation_preview(
                    identity,
                    &document,
                    std::path::Path::new(&project_root),
                    tolerance,
                    operation_preview,
                )
                .map(|result| WorkerResponse::Evaluation { result }),
            WorkerRequest::Export {
                identity,
                document,
                project_root,
                path,
                format,
                tolerance,
            } => evaluator
                .export(
                    &document,
                    std::path::Path::new(&project_root),
                    std::path::Path::new(&path),
                    &format,
                    tolerance,
                )
                .map(|()| WorkerResponse::Exported { identity, path }),
            WorkerRequest::Release {
                identity,
                document,
                project_root,
                path,
                tolerance,
            } => cad_core::release::create(
                &mut evaluator,
                &document,
                std::path::Path::new(&project_root),
                std::path::Path::new(&path),
                tolerance,
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
