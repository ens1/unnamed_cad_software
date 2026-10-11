use cad_core::{
    CadError, Document, Result,
    commands::Command,
    persistence::{FileStore, ProjectStore, atomic_write, project_root},
    protocol::{GeometryService, PROTOCOL_VERSION, RequestIdentity},
    version_control::{Git, VersionControl},
};
use cad_kernel::evaluate::Evaluator;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
#[command(
    name = "tcad",
    version,
    about = "Local parametric CAD: rebuild, edit, exchange and release the same desktop documents"
)]
struct Args {
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    New {
        path: PathBuf,
        #[arg(long, default_value = "Untitled assembly")]
        name: String,
        #[arg(long)]
        sample: bool,
    },
    Validate {
        project: PathBuf,
        #[arg(long, default_value_t = 0.1)]
        tolerance: f64,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Export {
        project: PathBuf,
        output: PathBuf,
        #[arg(long)]
        format: Option<String>,
        #[arg(long, default_value_t = 0.1)]
        tolerance: f64,
    },
    Edit {
        project: PathBuf,
        command: PathBuf,
        #[arg(long, default_value_t = 0.1)]
        tolerance: f64,
    },
    Bom {
        project: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Diff {
        before: PathBuf,
        after: PathBuf,
    },
    Merge {
        base: PathBuf,
        ours: PathBuf,
        theirs: PathBuf,
        output: PathBuf,
        #[arg(long, default_value_t = 0.1)]
        tolerance: f64,
    },
    Release {
        project: PathBuf,
        destination: PathBuf,
        #[arg(long, default_value_t = 0.1)]
        tolerance: f64,
    },
    Import {
        project: PathBuf,
        step: PathBuf,
        #[arg(long, default_value_t = 0.1)]
        tolerance: f64,
    },
    Describe {
        project: PathBuf,
    },
    Link {
        project: PathBuf,
        source: PathBuf,
        #[arg(long)]
        definition: Option<String>,
        #[arg(long)]
        update: Option<String>,
        #[arg(long, default_value_t = 0.1)]
        tolerance: f64,
    },
    Git {
        project: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
fn identity(d: &Document, e: &Evaluator) -> RequestIdentity {
    RequestIdentity {
        protocol: PROTOCOL_VERSION,
        generation: 0,
        base_revision: d.revision,
        draft: None,
        edit: 0,
        source: d.fingerprint(),
        backend: e.fingerprint().into(),
    }
}
fn checked(
    e: &mut Evaluator,
    d: &Document,
    root: &std::path::Path,
    t: f64,
) -> Result<cad_core::protocol::Evaluation> {
    e.evaluate(identity(d, e), d, root, t)
}
fn run() -> Result<()> {
    let args = Args::parse();
    let mut evaluator = Evaluator::default();
    match args.command {
        Action::New { path, name, sample } => {
            if path.exists() {
                return Err(CadError::Invalid("Destination already exists".into()));
            }
            let d = if sample {
                Document::sample()
            } else {
                Document::empty(&name)
            };
            FileStore.save(&path, &d)?;
            println!("Created {}", path.display());
        }
        Action::Validate {
            project,
            tolerance,
            output,
        } => {
            let d = FileStore.load(&project)?;
            let e = checked(&mut evaluator, &d, &project_root(&project), tolerance)?;
            if let Some(path) = output {
                atomic_write(&path, &serde_json::to_vec_pretty(&e)?)?;
            }
            println!(
                "Valid revision {}: {} definitions, {} instances, {} ms\nSource {}\nBackend {}",
                d.revision,
                e.meshes.len(),
                e.instances.len(),
                e.elapsed_ms,
                d.fingerprint(),
                e.identity.backend
            );
        }
        Action::Export {
            project,
            output,
            format,
            tolerance,
        } => {
            let d = FileStore.load(&project)?;
            let format = format
                .or_else(|| {
                    output
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(str::to_string)
                })
                .ok_or_else(|| {
                    CadError::Invalid("Choose an output extension or --format".into())
                })?;
            evaluator.export(&d, &project_root(&project), &output, &format, tolerance)?;
            println!("Exported {}", output.display());
        }
        Action::Edit {
            project,
            command,
            tolerance,
        } => {
            let mut d = FileStore.load(&project)?;
            let command: Command = serde_json::from_slice(&std::fs::read(command)?)?;
            command.execute(&mut d)?;
            checked(&mut evaluator, &d, &project_root(&project), tolerance)?;
            d.revision += 1;
            FileStore.save(&project, &d)?;
            println!("Saved revision {}", d.revision);
        }
        Action::Bom { project, output } => {
            let csv = cad_core::manufacturing::csv(&FileStore.load(&project)?)?;
            if let Some(path) = output {
                atomic_write(&path, csv.as_bytes())?;
            } else {
                print!("{csv}");
            }
        }
        Action::Diff { before, after } => {
            let changes = cad_core::version_control::diff(
                &FileStore.load(&before)?,
                &FileStore.load(&after)?,
            )?;
            println!("{}", serde_json::to_string_pretty(&changes)?);
        }
        Action::Merge {
            base,
            ours,
            theirs,
            output,
            tolerance,
        } => {
            let mut merged = cad_core::version_control::merge(
                &FileStore.load(&base)?,
                &FileStore.load(&ours)?,
                &FileStore.load(&theirs)?,
            )?;
            merged.revision += 1;
            let destination = project_root(&output);
            cad_core::persistence::copy_resources(
                &merged,
                &[
                    project_root(&ours),
                    project_root(&theirs),
                    project_root(&base),
                ],
                &destination,
            )?;
            checked(&mut evaluator, &merged, &destination, tolerance)?;
            FileStore.save(&output, &merged)?;
            println!("Merged {}", output.display());
        }
        Action::Release {
            project,
            destination,
            tolerance,
        } => {
            let m = cad_core::release::create(
                &mut evaluator,
                &FileStore.load(&project)?,
                &project_root(&project),
                &destination,
                tolerance,
            )?;
            println!(
                "Released {} from {}",
                destination.display(),
                m.source_sha256
            );
        }
        Action::Import {
            project,
            step,
            tolerance,
        } => {
            let mut d = FileStore.load(&project)?;
            let root = project_root(&project);
            let resource = cad_core::persistence::import_resource(&mut d, &root, &step)?;
            let imported = cad_kernel::exchange::import_structure(&mut d, &resource, &root)?;
            let key = cad_core::id();
            let name = cad_core::presentation::definition_name(&d, &imported);
            d.assemblies
                .get_mut(&d.root)
                .ok_or_else(|| CadError::Invalid("Import needs an assembly root".into()))?
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
            checked(&mut evaluator, &d, &root, tolerance)?;
            d.revision += 1;
            FileStore.save(&project, &d)?;
            println!("Imported {}", step.display());
        }
        Action::Describe { project } => {
            let d = FileStore.load(&project)?;
            let s = cad_core::Session::new(d, evaluator.fingerprint());
            println!(
                "{}",
                serde_json::to_string_pretty(&cad_core::presentation::project(&s, None))?
            );
        }
        Action::Link {
            project,
            source,
            definition,
            update,
            tolerance,
        } => {
            let mut d = FileStore.load(&project)?;
            let root = project_root(&project);
            let link = cad_core::library::link(
                &mut d,
                &root,
                &source,
                definition.as_deref(),
                update.as_deref(),
            )?;
            checked(&mut evaluator, &d, &root, tolerance)?;
            d.revision += 1;
            FileStore.save(&project, &d)?;
            println!("Pinned component link {link} at revision {}", d.revision);
        }
        Action::Git { project, args } => {
            if args.is_empty() {
                return Err(CadError::Invalid(
                    "Provide Git arguments, for example status --short".into(),
                ));
            }
            print!(
                "{}",
                Git.command(
                    &project_root(&project),
                    &args.iter().map(String::as_str).collect::<Vec<_>>()
                )?
            );
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
