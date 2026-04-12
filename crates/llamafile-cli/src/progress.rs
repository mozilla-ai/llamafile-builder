use indicatif::{ProgressBar, ProgressStyle};
use llamafile_core::builder::BuildEvent;
use llamafile_core::models::BuildStage;
use tokio::sync::mpsc;

/// Consume build events and display progress to stderr.
pub async fn display_progress(mut rx: mpsc::Receiver<BuildEvent>) {
    let pb = ProgressBar::new(100);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:30.cyan/blue}] {msg}")
            .unwrap()
            .progress_chars("=> "),
    );

    let download_pb = ProgressBar::new(0);
    download_pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.yellow} [{bar:30.yellow/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("=> "),
    );
    download_pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());

    while let Some(event) = rx.recv().await {
        match event {
            BuildEvent::Progress(progress) => {
                pb.set_position(progress.progress_pct as u64);
                let stage_name = match progress.stage {
                    BuildStage::DownloadingRuntime => "Downloading runtime",
                    BuildStage::DownloadingModel => "Downloading model",
                    BuildStage::WritingConfig => "Writing configuration",
                    BuildStage::BundlingGpu => "Preparing GPU libraries",
                    BuildStage::CreatingExecutable => "Creating executable",
                    BuildStage::Complete => "Complete",
                    BuildStage::Failed => "Failed",
                    BuildStage::Cancelled => "Cancelled",
                    BuildStage::Idle => "Idle",
                };
                if progress.message.is_empty() {
                    pb.set_message(stage_name.to_string());
                } else {
                    pb.set_message(progress.message);
                }
            }
            BuildEvent::Download(Some(dp)) => {
                if download_pb.is_hidden() {
                    download_pb.set_draw_target(indicatif::ProgressDrawTarget::stderr());
                }
                download_pb.set_length(dp.bytes_total);
                download_pb.set_position(dp.bytes_downloaded);
            }
            BuildEvent::Download(None) => {
                download_pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
            }
            BuildEvent::Log(msg) => {
                pb.println(format!("  {}", msg));
            }
            BuildEvent::Complete { output_path, size } => {
                pb.finish_with_message(format!(
                    "Done: {} ({})",
                    output_path.display(),
                    llamafile_core::models::format_size(size)
                ));
                download_pb.finish_and_clear();
                break;
            }
            BuildEvent::Failed { error } => {
                pb.abandon_with_message(format!("Failed: {}", error));
                download_pb.finish_and_clear();
                break;
            }
            BuildEvent::NeedDecision(_) => {
                // CLI doesn't handle interactive decisions -- SkipExisting policy
                // means we should never get here.
                pb.println("  (unexpected decision request in CLI mode)");
            }
        }
    }
}
