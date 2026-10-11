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
            WorkerRequest::Evaluate { identity, .. }
            | WorkerRequest::Export { identity, .. }
            | WorkerRequest::Release { identity, .. }
            | WorkerRequest::Import { identity, .. } => Some(identity.clone()),
            WorkerRequest::Shutdown => break,
        };
        let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match request {
            WorkerRequest::Evaluate {
                identity,
                document,
                project_root,
                tolerance,
            } => evaluator
                .evaluate(
                    identity,
                    &document,
                    std::path::Path::new(&project_root),
                    tolerance,
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
                            fixed: true,
                            translation_expressions: None,
                        },
                    );
                document.validate()?;
                Ok(WorkerResponse::Imported { identity, document })
            }
            WorkerRequest::Shutdown => unreachable!(),
        }));
        let response = match response {
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
        write_frame(&mut output, &response)?;
    }
    Ok(())
}
