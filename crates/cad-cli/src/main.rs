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
    /// Fork a part or assembly using a versioned fork intent JSON file.
    Fork {
        project: PathBuf,
        intent: PathBuf,
        #[arg(long)]
        apply: bool,
    },
    /// Review or apply selected features; unrelated source features remain unchanged.
    TransferFeatures {
        project: PathBuf,
        source: PathBuf,
        request: PathBuf,
        #[arg(long)]
        revision: Option<String>,
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Preview or accept a component pinned to an exact Git revision.
    LinkGit {
        project: PathBuf,
        request: PathBuf,
        #[arg(long)]
        update: Option<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Generate the marked interactive README preview from the saved model.
    ReadmePreview {
        project: PathBuf,
        #[arg(long = "display-tolerance", visible_alias = "tolerance")]
        tolerance: Option<f64>,
    },
    New {
        path: PathBuf,
        #[arg(long, default_value = "Untitled assembly")]
        name: String,
        #[arg(long)]
        sample: bool,
    },
    Validate {
        project: PathBuf,
        #[arg(
            long = "display-tolerance",
            visible_alias = "tolerance",
            help = "Override display/export chord tolerance in mm; modeling accuracy remains in the saved model policy"
        )]
        tolerance: Option<f64>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Export {
        project: PathBuf,
        output: PathBuf,
        #[arg(long)]
        format: Option<String>,
        #[arg(
            long = "display-tolerance",
            visible_alias = "tolerance",
            help = "Override display/export chord tolerance in mm; modeling accuracy remains in the saved model policy"
        )]
        tolerance: Option<f64>,
    },
    Edit {
        project: PathBuf,
        command: PathBuf,
        #[arg(
            long = "display-tolerance",
            visible_alias = "tolerance",
            help = "Override display/export chord tolerance in mm; modeling accuracy remains in the saved model policy"
        )]
        tolerance: Option<f64>,
    },
    Bom {
        project: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Sensors {
        project: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(
            long,
            help = "Read parameter monitors without rebuilding geometry; property monitors report unavailable"
        )]
        source_only: bool,
        #[arg(
            long = "display-tolerance",
            visible_alias = "tolerance",
            help = "Override display/export chord tolerance in mm; modeling accuracy remains in the saved model policy"
        )]
        tolerance: Option<f64>,
    },
    ToleranceStacks {
        project: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Materials {
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
        #[arg(
            long = "display-tolerance",
            visible_alias = "tolerance",
            help = "Override display/export chord tolerance in mm; modeling accuracy remains in the saved model policy"
        )]
        tolerance: Option<f64>,
    },
    Release {
        project: PathBuf,
        destination: PathBuf,
        #[arg(
            long = "display-tolerance",
            visible_alias = "tolerance",
            help = "Override display/export chord tolerance in mm; modeling accuracy remains in the saved model policy"
        )]
        tolerance: Option<f64>,
    },
    Import {
        project: PathBuf,
        step: PathBuf,
        #[arg(
            long = "display-tolerance",
            visible_alias = "tolerance",
            help = "Override display/export chord tolerance in mm; modeling accuracy remains in the saved model policy"
        )]
        tolerance: Option<f64>,
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
        #[arg(
            long = "display-tolerance",
            visible_alias = "tolerance",
            help = "Override display/export chord tolerance in mm; modeling accuracy remains in the saved model policy"
        )]
        tolerance: Option<f64>,
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
    tolerance: Option<f64>,
) -> Result<cad_core::protocol::Evaluation> {
    let evaluation = e.evaluate(
        identity(d, e),
        d,
        root,
        tolerance.unwrap_or(d.settings.display_tolerance_mm),
    )?;
    if !evaluation.valid {
        return Err(CadError::Invalid(
            evaluation
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    Ok(evaluation)
}
fn save_reviewed(
    project: &std::path::Path,
    starting: &Document,
    candidate: &Document,
) -> Result<()> {
    if FileStore.load(project)?.fingerprint() != starting.fingerprint() {
        return Err(CadError::Invalid(
            "The saved project changed during this operation; review the change again".into(),
        ));
    }
    FileStore.save(project, candidate)
}
fn manifest_name(project: &std::path::Path) -> Result<&str> {
    if project
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        project
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| CadError::Invalid("The project filename must use UTF-8".into()))
    } else {
        Ok("project.json")
    }
}
fn writable_source(project: &std::path::Path, apply: bool) -> Result<()> {
    if apply && cad_core::release::is_release_source(project) {
        return Err(CadError::Invalid(
            "Save a working copy before editing released source".into(),
        ));
    }
    Ok(())
}
fn run() -> Result<()> {
    let args = Args::parse();
    let mut evaluator = Evaluator::default();
    match args.command {
        Action::Fork {
            project,
            intent,
            apply,
        } => {
            writable_source(&project, apply)?;
            let document = FileStore.load(&project)?;
            let intent: cad_core::component_fork::Intent =
                serde_json::from_slice(&std::fs::read(intent)?)?;
            let plan = cad_core::component_fork::plan_with_resources(
                &document,
                &intent,
                &project_root(&project),
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"definition":plan.definition,"copied":plan.copied,"reused":plan.reused,"notices":plan.notices,"apply_requested":apply})
                )?
            );
            if apply {
                let mut candidate = plan.document;
                checked(&mut evaluator, &candidate, &project_root(&project), None)?;
                candidate.revision = document.revision + 1;
                save_reviewed(&project, &document, &candidate)?;
                println!("Fork saved as revision {}", candidate.revision);
            }
        }
        Action::TransferFeatures {
            project,
            source,
            request,
            revision,
            base,
            apply,
        } => {
            writable_source(&project, apply)?;
            let target = FileStore.load(&project)?;
            let request: cad_core::feature_transfer::Request =
                serde_json::from_slice(&std::fs::read(request)?)?;
            let source_root = project_root(&source);
            let manifest = manifest_name(&source)?;
            let revision = revision
                .as_deref()
                .map(|revision| cad_core::version_control::resolve_commit(&source_root, revision))
                .transpose()?;
            let source_document = if let Some(revision) = &revision {
                cad_core::version_control::snapshot_manifest(&source_root, revision, manifest)?
            } else {
                FileStore.load(&source)?
            };
            let base = base
                .as_ref()
                .map(|revision| {
                    cad_core::version_control::snapshot_manifest(&source_root, revision, manifest)
                })
                .transpose()?;
            let plan = cad_core::feature_transfer::prepare(
                &source_document,
                base.as_ref(),
                &target,
                &request,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"items":plan.items,"conflicts":plan.conflicts,"diagnostics":plan.diagnostics,"ready":plan.candidate.is_some(),"apply_requested":apply})
                )?
            );
            if apply {
                let mut candidate = plan.document(&target)?;
                let root = project_root(&project);
                if let Some(revision) = revision {
                    let staging = std::env::temp_dir()
                        .join(format!("tcad-feature-transfer-{}", cad_core::id()));
                    let result = (|| -> Result<()> {
                        for resource in &plan.resources {
                            cad_core::version_control::snapshot_resource(
                                &source_root,
                                &revision,
                                &resource.source,
                                &staging,
                            )?;
                        }
                        cad_core::feature_transfer::materialize(&plan, &staging, &root)
                    })();
                    let _ = std::fs::remove_dir_all(&staging);
                    result?;
                } else {
                    cad_core::feature_transfer::materialize(&plan, &source_root, &root)?;
                }
                checked(&mut evaluator, &candidate, &root, None)?;
                candidate.revision = target.revision + 1;
                save_reviewed(&project, &target, &candidate)?;
                println!("Selected features saved as revision {}", candidate.revision);
            }
        }
        Action::LinkGit {
            project,
            request,
            update,
            apply,
        } => {
            writable_source(&project, apply)?;
            let source = FileStore.load(&project)?;
            let mut candidate = source.clone();
            let request: cad_core::library::GitLinkRequest =
                serde_json::from_slice(&std::fs::read(request)?)?;
            let root = project_root(&project);
            let link =
                cad_core::library::link_git(&mut candidate, &root, &request, update.as_deref())?;
            let changes = cad_core::version_control::diff(&source, &candidate)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"link":link,"changes":changes,"apply_requested":apply})
                )?
            );
            if apply {
                checked(&mut evaluator, &candidate, &root, None)?;
                candidate.revision = source.revision + 1;
                save_reviewed(&project, &source, &candidate)?;
                println!(
                    "Pinned Git component saved as revision {}",
                    candidate.revision
                );
            }
        }
        Action::ReadmePreview { project, tolerance } => {
            let document = FileStore.load(&project)?;
            let evaluation = checked(
                &mut evaluator,
                &document,
                &project_root(&project),
                tolerance,
            )?;
            let preview = cad_core::readme_preview::build(&document, &evaluation)?;
            cad_core::readme_preview::write(&project, &document, &preview)?;
            println!(
                "Updated README model preview: {} triangles, source {}",
                preview.metadata.triangles, preview.metadata.source_sha256
            );
        }
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
            evaluator.export(
                &d,
                &project_root(&project),
                &output,
                &format,
                tolerance.unwrap_or(d.settings.display_tolerance_mm),
            )?;
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
            let document = FileStore.load(&project)?;
            let csv = cad_core::bom::csv(&document, &document.settings.production.bom)?;
            if let Some(path) = output {
                atomic_write(&path, csv.as_bytes())?;
            } else {
                print!("{csv}");
            }
        }
        Action::Materials { project, output } => {
            let document = FileStore.load(&project)?;
            let catalog = cad_core::material_library::Catalog::new(
                format!("{} materials", document.name),
                document.materials.into_values().collect(),
            );
            let csv = output.as_ref().is_some_and(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
            });
            let source = if csv {
                cad_core::material_csv::export(&catalog)?
            } else {
                cad_core::material_library::export_json(&catalog)?
            };
            if let Some(path) = output {
                atomic_write(&path, source.as_bytes())?;
                println!("Exported material catalog {}", path.display());
            } else {
                println!("{source}");
            }
        }
        Action::ToleranceStacks { project, output } => {
            let document = FileStore.load(&project)?;
            let report = cad_core::tolerance_stacks::evaluate(&document);
            let json = serde_json::to_vec_pretty(&report)?;
            if let Some(path) = output {
                atomic_write(&path, &json)?;
                println!("Exported tolerance stacks {}", path.display());
            } else {
                println!("{}", String::from_utf8_lossy(&json));
            }
        }
        Action::Sensors {
            project,
            output,
            source_only,
            tolerance,
        } => {
            let document = FileStore.load(&project)?;
            let evaluation = if source_only {
                None
            } else {
                Some(checked(
                    &mut evaluator,
                    &document,
                    &project_root(&project),
                    tolerance,
                )?)
            };
            let report = cad_core::sensors::evaluate(&document, evaluation.as_ref());
            let data = serde_json::to_vec_pretty(&report)?;
            if let Some(path) = output {
                atomic_write(&path, &data)?;
            } else {
                println!("{}", String::from_utf8_lossy(&data));
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
            let d = FileStore.load(&project)?;
            let m = cad_core::release::create(
                &mut evaluator,
                &d,
                &project_root(&project),
                &destination,
                tolerance.unwrap_or(d.settings.display_tolerance_mm),
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
                        finish: None,
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
