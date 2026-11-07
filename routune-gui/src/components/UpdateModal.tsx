import { useState } from "react";
import {
  Sparkles,
  ArrowRight,
  DownloadCloud,
  CheckCircle2,
  AlertCircle,
  ExternalLink,
  Copy,
  Check,
  RefreshCw,
  X,
} from "lucide-react";
import {
  ReleaseInfo,
  runInAppUpdate,
  restartApp,
  openExternalUrl,
} from "../services/updateService";

interface UpdateModalProps {
  isOpen: boolean;
  onClose: () => void;
  release: ReleaseInfo | null;
  currentVersion: string;
}

export function UpdateModal({
  isOpen,
  onClose,
  release,
  currentVersion,
}: UpdateModalProps) {
  const [status, setStatus] = useState<"idle" | "updating" | "success" | "error">("idle");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  if (!isOpen || !release) {
    return null;
  }

  const manualCommand = "curl -fsSL https://raw.githubusercontent.com/Nekoplii/Routune/main/install.sh | bash";

  const handleStartUpdate = async () => {
    setStatus("updating");
    setErrorMessage(null);

    try {
      await runInAppUpdate();
      setStatus("success");
    } catch (err) {
      setStatus("error");
      setErrorMessage(
        typeof err === "string"
          ? err
          : err instanceof Error
            ? err.message
            : "Unknown updater error"
      );
    }
  };

  const handleCopyCommand = () => {
    navigator.clipboard.writeText(manualCommand).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  const handleDismiss = () => {
    if (status === "updating") return;
    // Remember dismissed release tag for this session/version
    try {
      localStorage.setItem("routune_dismissed_release", release.tagName);
    } catch {
      // ignore
    }
    onClose();
  };

  const displayCurrent = currentVersion.startsWith("v")
    ? currentVersion
    : `v${currentVersion}`;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-md p-4 select-none animate-in fade-in duration-200">
      <div className="relative w-full max-w-lg rounded-2xl border-2 border-[#2B2942] bg-[#171528] p-6 shadow-2xl shadow-black/80 flex flex-col gap-5 text-[#ECEAFB]">
        {/* Top Header */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-center gap-3">
            <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-[#7C6AF2]/15 border-2 border-[#7C6AF2]/30 text-[#7C6AF2]">
              <Sparkles className="h-6 w-6" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h3 className="text-base font-bold tracking-tight text-[#ECEAFB]">
                  Update Available
                </h3>
                <span className="rounded-full bg-[#7C6AF2]/20 border border-[#7C6AF2]/40 px-2 py-0.5 text-[10px] font-mono font-semibold text-[#C4B5FD]">
                  NEW
                </span>
              </div>
              <p className="text-xs text-[#ECEAFB]/60 mt-0.5">
                A new version of Routune Studio is available.
              </p>
            </div>
          </div>

          <button
            type="button"
            onClick={handleDismiss}
            disabled={status === "updating"}
            className="flex h-8 w-8 items-center justify-center rounded-lg text-[#ECEAFB]/50 hover:bg-[#201D36] hover:text-[#ECEAFB] transition-colors cursor-pointer disabled:opacity-30 disabled:pointer-events-none"
            title="Dismiss"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Version Diff Banner */}
        <div className="flex items-center justify-between rounded-xl border border-[#2B2942] bg-[#0F0E17] px-4 py-3">
          <div className="flex flex-col">
            <span className="text-[10px] uppercase tracking-wider font-mono text-[#ECEAFB]/40">
              Installed
            </span>
            <span className="text-xs font-mono font-medium text-[#ECEAFB]/70">
              {displayCurrent}
            </span>
          </div>

          <ArrowRight className="h-4 w-4 text-[#7C6AF2]" />

          <div className="flex flex-col items-end">
            <span className="text-[10px] uppercase tracking-wider font-mono text-[#7C6AF2]">
              Latest Release
            </span>
            <span className="text-xs font-mono font-bold text-[#C4B5FD]">
              {release.tagName}
            </span>
          </div>
        </div>

        {/* Release Notes */}
        {release.body && (
          <div className="flex flex-col gap-1.5">
            <span className="text-[11px] font-semibold text-[#ECEAFB]/60 uppercase tracking-wider">
              Release Notes
            </span>
            <div className="max-h-40 overflow-y-auto rounded-xl border border-[#2B2942] bg-[#0F0E17]/80 p-3 text-xs text-[#ECEAFB]/80 font-sans leading-relaxed whitespace-pre-wrap select-text">
              {release.body}
            </div>
          </div>
        )}

        {/* Status Indicators */}
        {status === "updating" && (
          <div className="flex items-center gap-3 rounded-xl border border-[#7C6AF2]/30 bg-[#7C6AF2]/10 p-3.5 text-xs text-[#C4B5FD]">
            <RefreshCw className="h-4 w-4 animate-spin text-[#7C6AF2] shrink-0" />
            <span>
              Downloading release and updating binaries... This usually takes a few seconds.
            </span>
          </div>
        )}

        {status === "success" && (
          <div className="flex items-start gap-3 rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-3.5 text-xs text-emerald-300">
            <CheckCircle2 className="h-4 w-4 text-emerald-400 shrink-0 mt-0.5" />
            <div className="flex flex-col gap-1">
              <span className="font-semibold">Update completed successfully!</span>
              <span className="text-emerald-300/80">
                Daemon and GUI packages updated. Restart Routune to start running {release.tagName}.
              </span>
            </div>
          </div>
        )}

        {status === "error" && (
          <div className="flex flex-col gap-2 rounded-xl border border-rose-500/30 bg-rose-500/10 p-3.5 text-xs text-rose-300">
            <div className="flex items-center gap-2 font-semibold text-rose-300">
              <AlertCircle className="h-4 w-4 text-rose-400 shrink-0" />
              <span>In-app update encountered an error</span>
            </div>
            {errorMessage && (
              <span className="text-[11px] font-mono text-rose-300/80 whitespace-pre-wrap break-all">
                {errorMessage}
              </span>
            )}
            <div className="mt-1 flex items-center justify-between rounded-lg bg-[#0F0E17] border border-rose-500/20 px-2.5 py-1.5 text-[11px] font-mono">
              <span className="truncate pr-2 text-[#ECEAFB]/70">{manualCommand}</span>
              <button
                type="button"
                onClick={handleCopyCommand}
                className="flex items-center gap-1 text-[#C4B5FD] hover:text-white cursor-pointer shrink-0"
              >
                {copied ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                <span>{copied ? "Copied" : "Copy"}</span>
              </button>
            </div>
          </div>
        )}

        {/* Modal Action Buttons */}
        <div className="flex items-center justify-between gap-3 pt-1">
          <button
            type="button"
            onClick={() => openExternalUrl(release.htmlUrl)}
            className="flex items-center gap-1.5 text-xs text-[#ECEAFB]/60 hover:text-[#C4B5FD] transition-colors cursor-pointer"
          >
            <ExternalLink className="h-3.5 w-3.5" />
            <span>GitHub Release</span>
          </button>

          <div className="flex items-center gap-2">
            {status === "idle" && (
              <>
                <button
                  type="button"
                  onClick={handleDismiss}
                  className="rounded-xl border border-[#2B2942] bg-[#201D36] px-3.5 py-2 text-xs font-semibold text-[#ECEAFB]/80 hover:bg-[#2B2942] hover:text-[#ECEAFB] transition-colors cursor-pointer"
                >
                  Remind Me Later
                </button>
                <button
                  type="button"
                  onClick={handleStartUpdate}
                  className="flex items-center gap-2 rounded-xl bg-[#7C6AF2] hover:bg-[#6C59E3] border border-[#7C6AF2] px-4 py-2 text-xs font-semibold text-white shadow-md shadow-[#7C6AF2]/25 transition-all cursor-pointer"
                >
                  <DownloadCloud className="h-4 w-4" />
                  <span>Update Now</span>
                </button>
              </>
            )}

            {status === "updating" && (
              <button
                type="button"
                disabled
                className="flex items-center gap-2 rounded-xl bg-[#7C6AF2]/50 border border-[#7C6AF2]/40 px-4 py-2 text-xs font-semibold text-white/70 cursor-not-allowed"
              >
                <RefreshCw className="h-4 w-4 animate-spin" />
                <span>Updating...</span>
              </button>
            )}

            {status === "success" && (
              <button
                type="button"
                onClick={() => restartApp()}
                className="flex items-center gap-2 rounded-xl bg-emerald-600 hover:bg-emerald-500 border border-emerald-500 px-4 py-2 text-xs font-semibold text-white shadow-md shadow-emerald-600/25 transition-all cursor-pointer"
              >
                <RefreshCw className="h-4 w-4" />
                <span>Restart Routune</span>
              </button>
            )}

            {status === "error" && (
              <>
                <button
                  type="button"
                  onClick={handleDismiss}
                  className="rounded-xl border border-[#2B2942] bg-[#201D36] px-3.5 py-2 text-xs font-semibold text-[#ECEAFB]/80 hover:bg-[#2B2942] transition-colors cursor-pointer"
                >
                  Close
                </button>
                <button
                  type="button"
                  onClick={handleStartUpdate}
                  className="flex items-center gap-2 rounded-xl bg-[#7C6AF2] hover:bg-[#6C59E3] border border-[#7C6AF2] px-4 py-2 text-xs font-semibold text-white transition-all cursor-pointer"
                >
                  <RefreshCw className="h-4 w-4" />
                  <span>Retry</span>
                </button>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
