// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import crystal from "../assets/crispawn/ui/crystal/CRISPAWN_00.png";
import wordmark from "../assets/crispawn/ui/wordmark.png";
import { strings } from "./strings";
import "./styles.css";

type Language = keyof typeof strings;
type Step = 0 | 1 | 2 | 3 | 4;
type UsbDisk = { diskNumber: number; friendlyName: string; uniqueId: string; diskPath: string; serialNumber: string;
  sizeBytes: number; driveLetter: string | null; layoutStamp: string; prepared: boolean; eligible: boolean;
  issue: keyof typeof strings.ja.usbIssue | null };
type InternalDisk = { diskNumber: number; friendlyName: string; sizeBytes: number; busType: string };
type UsbPhase = keyof typeof strings.ja.usbProgress;
type UsbProgress = { phase: UsbPhase; percent: number };
type ReleaseSupport = { tag: string; configVersions: number[]; capabilities: string[] };
type RuntimeInput = {
  backupDirectory: string; startupMode: string; uiMode: string; failureAction: string;
  logPolicy: string; logSyncIntervalMs: string; initialProfile: string;
  verifyAfterWrite: boolean; probeIntervalMs: string;
  ioPolicy: { noProgressSeconds: string; failureGraceSeconds: string; shutdownGraceSeconds: string;
    transientRetries: string; retryDelayMs: string };
};

function ChoiceIcon({ kind }: { kind: "disk" | "usb" }) {
  return <span className="choice-icon" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none"
    stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    {kind === "disk" && <><rect x="3" y="5" width="18" height="14" rx="3" /><path d="M7 10h10M7 15h.01M11 15h.01" /></>}
    {kind === "usb" && <><path d="M7 3h10v5H7zM5 8h14v12H5zM9 16h6" /></>}
  </svg></span>;
}

function usbKey(disk: UsbDisk) { return `${disk.diskNumber}:${disk.diskPath}`; }

function App() {
  const [step, setStep] = useState<Step>(0);
  const [locale, setLocale] = useState<Language>("ja");
  const [language, setLanguage] = useState<Language>("ja");
  const [pcMode, setPcMode] = useState<"local" | "other">("local");
  const [internalDisks, setInternalDisks] = useState<InternalDisk[]>([]);
  const [selectedDiskNumbers, setSelectedDiskNumbers] = useState<number[]>([]);
  const [start, setStart] = useState("30");
  const [off, setOff] = useState("5");
  const [stay, setStay] = useState(false);
  const [options, setOptions] = useState<RuntimeInput>({
    backupDirectory: "crispawn/backups", startupMode: "auto_copy", uiMode: "gui", failureAction: "poweroff",
    logPolicy: "always", logSyncIntervalMs: "0", initialProfile: "balanced", verifyAfterWrite: true,
    probeIntervalMs: "250", ioPolicy: { noProgressSeconds: "180", failureGraceSeconds: "5",
      shutdownGraceSeconds: "20", transientRetries: "3", retryDelayMs: "100" },
  });
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [output, setOutput] = useState("usb");
  const [usbDisks, setUsbDisks] = useState<UsbDisk[]>([]);
  const [usbDiskId, setUsbDiskId] = useState("");
  const [reinitializeUsb, setReinitializeUsb] = useState(false);
  const [usbProgress, setUsbProgress] = useState<UsbProgress | null>(null);
  const [sourceIsoVersion, setSourceIsoVersion] = useState<string | null>(null);
  const [releaseSupport, setReleaseSupport] = useState<ReleaseSupport | null>(null);
  const [releaseSupportError, setReleaseSupportError] = useState("");
  const [releaseSupportLoading, setReleaseSupportLoading] = useState(false);
  const t = strings[locale];
  const selectedUsb = usbDisks.find(disk => usbKey(disk) === usbDiskId);
  const writeStage = (usbProgress?.percent ?? 0) >= 73 ? 2 : (usbProgress?.percent ?? 0) >= 25 ? 1 : 0;
  const sourceIds: string[] = [];
  const sourceSummary = pcMode === "other" ? t.all : internalDisks
    .filter(disk => selectedDiskNumbers.includes(disk.diskNumber))
    .map(disk => disk.friendlyName).join(", ") || t.noSelectedDisks;
  function setOption<K extends keyof RuntimeInput>(key: K, value: RuntimeInput[K]) {
    setOptions(current => ({ ...current, [key]: value }));
  }
  function setIoOption<K extends keyof RuntimeInput["ioPolicy"]>(key: K, value: RuntimeInput["ioPolicy"][K]) {
    setOptions(current => ({ ...current, ioPolicy: { ...current.ioPolicy, [key]: value } }));
  }
  const supports = (capability: string) => !!releaseSupport?.configVersions.includes(2)
    && releaseSupport.capabilities.includes(capability);

  useEffect(() => {
    if (step === 2 && !releaseSupport && !releaseSupportError && !releaseSupportLoading) void refreshReleaseSupport();
  }, [step]);

  async function refreshReleaseSupport() {
    setReleaseSupportLoading(true);
    setReleaseSupportError("");
    try {
      setReleaseSupport(await invoke<ReleaseSupport>("release_support"));
    } catch (failure) {
      setReleaseSupportError(String(failure));
    } finally {
      setReleaseSupportLoading(false);
    }
  }

  useEffect(() => {
    void refreshUsb();
    void refreshInternal();
  }, []);

  async function refreshInternal() {
    try {
      const disks = await invoke<InternalDisk[]>("internal_disks");
      setInternalDisks(disks);
      setSelectedDiskNumbers(current => current.length || internalDisks.length
        ? current.filter(number => disks.some(disk => disk.diskNumber === number))
        : disks.map(disk => disk.diskNumber));
    } catch (failure) {
      setInternalDisks([]);
      setSelectedDiskNumbers([]);
      setError(String(failure));
    }
  }

  async function refreshUsb() {
    try {
      const disks = await invoke<UsbDisk[]>("usb_disks");
      setUsbDisks(disks);
      setUsbDiskId(current => disks.some(disk => usbKey(disk) === current && disk.eligible)
        ? current : (disks.find(disk => disk.eligible) ? usbKey(disks.find(disk => disk.eligible)!) : ""));
    } catch (failure) {
      setUsbDisks([]);
      setUsbDiskId("");
      setError(String(failure));
    }
  }

  function clearResult() {
    setMessage("");
    setError("");
    setUsbProgress(null);
    setSourceIsoVersion(null);
  }

  function go(next: Step) {
    clearResult();
    setStep(next);
  }

  async function submit() {
    clearResult();
    const within = (value: string, minimum: number, maximum: number) =>
      /^\d+$/.test(value) && Number(value) >= minimum && Number(value) <= maximum;
    const io = options.ioPolicy;
    if (!options.backupDirectory.trim()
      || (options.startupMode === "auto_copy" && !within(start, 0, 300))
      || (!stay && !within(off, 0, 60))
      || !within(options.logSyncIntervalMs, 0, 60000)
      || !within(options.probeIntervalMs, 100, 5000)
      || !within(io.noProgressSeconds, 1, 3600)
      || !within(io.failureGraceSeconds, 1, 120)
      || !within(io.shutdownGraceSeconds, 1, 120)
      || !within(io.transientRetries, 0, 8)
      || !within(io.retryDelayMs, 1, 2000)) {
      setError(t.invalidSettings);
      return;
    }
    if (pcMode === "local" && !selectedDiskNumbers.length) {
      setError(t.noSelectedDisks);
      return;
    }
    if (pcMode === "local" && selectedDiskNumbers.length !== internalDisks.length) {
      setError(t.sourceSelectionUnsupported);
      return;
    }
    const settings = {
      sourceIds,
      language,
      startSeconds: options.startupMode === "auto_copy" ? Number(start) : 0,
      poweroffSeconds: stay ? null : Number(off),
      options: {
        ...options,
        logSyncIntervalMs: Number(options.logSyncIntervalMs),
        probeIntervalMs: Number(options.probeIntervalMs),
        ioPolicy: {
          noProgressSeconds: Number(options.ioPolicy.noProgressSeconds),
          failureGraceSeconds: Number(options.ioPolicy.failureGraceSeconds),
          shutdownGraceSeconds: Number(options.ioPolicy.shutdownGraceSeconds),
          transientRetries: Number(options.ioPolicy.transientRetries),
          retryDelayMs: Number(options.ioPolicy.retryDelayMs),
        },
      },
    };
    setPending(true);
    let unlisten: (() => void) | undefined;
    try {
      if (output === "usb") {
        const disk = selectedUsb;
        if (!disk || !disk.eligible) throw new Error(t.usbSelectDisk);
        setUsbProgress({ phase: "checkingRelease", percent: 0 });
        unlisten = await listen<UsbProgress>("usb-write-progress", event => {
          setUsbProgress(event.payload);
          setStep(3);
        });
        const releaseVersion = await invoke<string | null>("save_usb", { disk, reinitializeUsb, settings, pcMode,
          selectedDiskNumbers, uiLanguage: locale });
        if (releaseVersion) {
          setUsbProgress({ phase: "complete", percent: 100 });
          setSourceIsoVersion(releaseVersion);
          setMessage(t.usbSaved);
          setStep(4);
        } else {
          setUsbProgress(null);
          setStep(2);
        }
      } else if (output === "iso") {
        setStep(3);
        setUsbProgress({ phase: "checkingRelease", percent: 0 });
        unlisten = await listen<UsbProgress>("usb-write-progress", event => setUsbProgress(event.payload));
        if (await invoke<boolean>("save_iso", {
          settings,
        })) {
          setMessage(t.isoSaved);
          setStep(4);
        } else setStep(2);
      }
    } catch (failure) {
      setError(String(failure));
    } finally {
      unlisten?.();
      setPending(false);
    }
  }

  return <>
    <header>
      <div className="brand">
        <img className="crystal" src={crystal} alt="" />
        <img className="wordmark" src={wordmark} alt="CRISPAWN" />
        <span>Builder</span>
      </div>
      <select aria-label={t.uiLanguage} value={locale} disabled={pending || step >= 3} onChange={event => {
        const selected = event.target.value as Language;
        document.documentElement.lang = selected;
        setLocale(selected);
        clearResult();
      }}>
        <option value="ja">日本語</option><option value="en">English</option>
      </select>
    </header>
    <form noValidate onSubmit={event => {
      event.preventDefault();
      if (step === 2) {
        void submit();
      } else if (step === 1) {
        go(2);
      } else if (step === 0) {
        go(1);
      }
    }}>
      <div className={step >= 2 ? "workspace review" : "workspace"}>
        <nav aria-label={t.steps.join(" / ")}>
          {t.steps.map((label, index) => <button key={label} type="button"
            aria-current={index === step ? "step" : undefined}
            disabled={pending || index > step || index >= 3} onClick={() => go(index as Step)}>
            <span>{index + 1}</span>{label}
          </button>)}
        </nav>
        <main>
          <h1>{t.steps[step]}</h1>
          <fieldset disabled={pending} onChange={clearResult}>
            {step === 0 && <>
              <label>{t.pc}<select value={pcMode} onChange={event => setPcMode(event.target.value as "local" | "other")}>
                <option value="local">{t.local}</option>
                <option value="other">{t.other}</option>
              </select></label>
              {pcMode === "local" && <section className="disk-selection">
                <h2>{t.disks}</h2>
                {internalDisks.length === 0 && <p>{t.noInternalDisks}</p>}
                {internalDisks.map(disk => <label className={`choice-card disk-card${selectedDiskNumbers.includes(disk.diskNumber) ? " selected" : ""}`} key={disk.diskNumber}>
                  <ChoiceIcon kind="disk" />
                  <span className="choice-copy"><strong>{disk.friendlyName}</strong>
                    <small>{t.diskNumber} {disk.diskNumber} · {disk.busType}</small></span>
                  <span className="disk-capacity">{Math.round(disk.sizeBytes / 1_000_000_000)} GB</span>
                  <input type="checkbox" checked={selectedDiskNumbers.includes(disk.diskNumber)}
                    onChange={event => setSelectedDiskNumbers(current => event.target.checked
                      ? [...current, disk.diskNumber] : current.filter(number => number !== disk.diskNumber))} />
                </label>)}
                <button type="button" onClick={() => void refreshInternal()}>{t.diskRefresh}</button>
              </section>}
              <p>{t.exclusion}</p>
            </>}
            {step === 1 && <>
              <section className="destination-choice" aria-label={t.destination}>
                <h2>{t.destination}</h2>
                <div className="destination-grid">
                  <label className="choice-card destination-card selected">
                    <ChoiceIcon kind="usb" /><input type="radio" name="destination" value="usb" checked readOnly />
                    <span className="choice-copy"><strong>{t.usb}</strong><small>{t.usbDestinationHint}</small></span>
                  </label>
                </div>
              </section>
              <p>{t.storage}</p>
            </>}
            {step === 2 && <div className="review-columns">
              <section>
                <Summary />
                <div className="release-support">
                  <span>{releaseSupport ? `${t.sourceIsoVersion}: ${releaseSupport.tag}`
                    : releaseSupportLoading ? t.checkingReleaseSupport : t.releaseSupportUnavailable}</span>
                  <button type="button" disabled={releaseSupportLoading} onClick={() => void refreshReleaseSupport()}>{t.checkReleaseSupport}</button>
                </div>
                {releaseSupportError && <p className="setting-note">{releaseSupportError}</p>}
                <h2>{t.start}</h2>
                <select aria-label={t.start} value={options.startupMode}
                  onChange={event => setOption("startupMode", event.target.value)}>
                  <option value="auto_copy">{t.auto}</option>
                  <option value="menu" disabled={options.uiMode === "headless" || !supports("startup.menu")}>{t.menu}</option>
                  <option value="restore_select" disabled={options.uiMode === "headless" || !supports("startup.restore_select")}>{t.restore}</option>
                </select>
                <div className="pair">
                  {options.startupMode === "auto_copy" && <label>{t.startDelay}
                    <input type="number" min="0" max="300" required value={start}
                      onChange={event => setStart(event.target.value)} />
                  </label>}
                  <label>{t.language}
                    <select value={language} onChange={event => setLanguage(event.target.value as Language)}>
                      <option value="ja">日本語</option><option value="en">English</option>
                    </select>
                  </label>
                </div>
                <h2>{t.complete}</h2>
                <div className="pair">
                  <select aria-label={t.complete} value={stay ? "stay" : "off"}
                    onChange={event => setStay(event.target.value === "stay")}>
                    <option value="off">{t.off}</option><option value="stay">{t.stay}</option>
                  </select>
                  {!stay && <label>{t.offDelay}
                    <input type="number" min="0" max="60" required value={off}
                      onChange={event => setOff(event.target.value)} />
                  </label>}
                </div>
                <section className="backup-verification" aria-label={t.backupVerification}>
                  <h2>{t.backupVerification}</h2>
                  <label className={`choice-card verification-card${options.verifyAfterWrite ? " selected" : ""}`}>
                    <span className="choice-icon" aria-hidden="true">✓</span>
                    <span className="choice-copy"><strong>{t.verifyOn}</strong><small>{t.verifyOnHelp}</small></span>
                    <input type="radio" name="backupVerification" checked={options.verifyAfterWrite}
                      onChange={() => setOption("verifyAfterWrite", true)} />
                  </label>
                  <label className={`choice-card verification-card${!options.verifyAfterWrite ? " selected" : ""}`}>
                    <span className="choice-icon" aria-hidden="true">»</span>
                    <span className="choice-copy"><strong>{t.verifyOff}</strong><small>{t.verifyOffHelp}
                      {!supports("processing.verify_after_write") && ` ${t.verifyUnavailable}`}</small></span>
                    <input type="radio" name="backupVerification" checked={!options.verifyAfterWrite}
                      disabled={!supports("processing.verify_after_write")}
                      onChange={() => setOption("verifyAfterWrite", false)} />
                  </label>
                </section>
                <details className="advanced-settings">
                  <summary>{t.advancedSettings}</summary>
                  <fieldset className="advanced-settings-body" disabled={!releaseSupport?.configVersions.includes(2)}>
                    {!releaseSupport?.configVersions.includes(2) && <p className="setting-note">{t.advancedUnavailable}</p>}
                    <label>{t.backupDirectory}<input type="text" maxLength={512} required value={options.backupDirectory}
                      onChange={event => setOption("backupDirectory", event.target.value)} /></label>
                    <div className="pair">
                      <label>{t.interfaceMode}<select value={options.uiMode} onChange={event => {
                        const mode = event.target.value;
                        setOptions(current => ({ ...current, uiMode: mode,
                          startupMode: mode === "headless" ? "auto_copy" : current.startupMode }));
                        if (mode !== "gui") setStart("0");
                      }}>
                        <option value="gui">{t.interfaceGui}</option>
                        <option value="console" disabled={!supports("ui.console")}>{t.interfaceConsole}</option>
                        <option value="headless" disabled={!supports("ui.headless")}>{t.interfaceHeadless}</option>
                      </select></label>
                      <label>{t.failureAction}<select value={options.failureAction}
                        onChange={event => setOption("failureAction", event.target.value)}>
                        <option value="poweroff">{t.failurePoweroff}</option>
                        <option value="stay" disabled={!supports("failure.stay")}>{t.failureStay}</option>
                      </select></label>
                    </div>
                    <div className="pair">
                      <label>{t.logPolicy}<select value={options.logPolicy}
                        onChange={event => setOption("logPolicy", event.target.value)}>
                        <option value="always">{t.logAlways}</option>
                        <option value="on_error" disabled={!supports("logging.on_error")}>{t.logOnError}</option>
                        <option value="never" disabled={!supports("logging.never")}>{t.logNever}</option>
                      </select></label>
                      <label>{t.logSyncInterval}<input type="number" min="0" max="60000" required
                        value={options.logSyncIntervalMs} onChange={event => setOption("logSyncIntervalMs", event.target.value)} /></label>
                    </div>
                    <label>{t.processingProfile}<select value={options.initialProfile}
                      onChange={event => setOption("initialProfile", event.target.value)}>
                      <option value="balanced">{t.profileBalanced}</option>
                      <option value="long_matches" disabled={!supports("processing.profile.long_matches")}>{t.profileLongMatches}</option>
                      <option value="high_compression" disabled={!supports("processing.profile.high_compression")}>{t.profileHighCompression}</option>
                      <option value="incompressible" disabled={!supports("processing.profile.incompressible")}>{t.profileIncompressible}</option>
                    </select></label>
                    <h3>{t.ioPolicy}</h3>
                    <div className="advanced-number-grid">
                      <label>{t.noProgressSeconds}<input type="number" min="1" max="3600" required value={options.ioPolicy.noProgressSeconds}
                        onChange={event => setIoOption("noProgressSeconds", event.target.value)} /></label>
                      <label>{t.failureGraceSeconds}<input type="number" min="1" max="120" required value={options.ioPolicy.failureGraceSeconds}
                        onChange={event => setIoOption("failureGraceSeconds", event.target.value)} /></label>
                      <label>{t.shutdownGraceSeconds}<input type="number" min="1" max="120" required value={options.ioPolicy.shutdownGraceSeconds}
                        onChange={event => setIoOption("shutdownGraceSeconds", event.target.value)} /></label>
                      <label>{t.transientRetries}<input type="number" min="0" max="8" required value={options.ioPolicy.transientRetries}
                        onChange={event => setIoOption("transientRetries", event.target.value)} /></label>
                      <label>{t.retryDelayMs}<input type="number" min="1" max="2000" required value={options.ioPolicy.retryDelayMs}
                        onChange={event => setIoOption("retryDelayMs", event.target.value)} /></label>
                      <label>{t.probeIntervalMs}<input type="number" min="100" max="5000" required value={options.probeIntervalMs}
                        onChange={event => setOption("probeIntervalMs", event.target.value)} /></label>
                    </div>
                  </fieldset>
                </details>
              </section>
              <section className="output">
                <h2>{t.output}</h2>
                <select aria-label={t.output} value={output} onChange={event => setOutput(event.target.value)}>
                  <option value="iso">{t.iso}</option>
                  <option value="usb">{t.bootUsb}</option>
                </select>
                {output === "iso" ? <>
                  <p>{t.isoHelp}</p>
                </> : output === "usb" ? <>
                  <h3 className="usb-heading">{t.usbDisk}</h3>
                  <div className="usb-target-list" role="radiogroup" aria-label={t.usbDisk}>
                    {usbDisks.length === 0 && <p>{t.usbNoDisk}</p>}
                    {usbDisks.map(disk => <label key={usbKey(disk)}
                      className={`choice-card usb-card${usbDiskId === usbKey(disk) ? " selected" : ""}`}>
                      <ChoiceIcon kind="usb" />
                      <span className="choice-copy"><strong>{disk.friendlyName}</strong>
                        <small>{t.diskNumber} {disk.diskNumber} · {Math.round(disk.sizeBytes / 1_000_000_000)} GB · {disk.eligible
                          ? disk.prepared ? t.usbPrepared : t.usbNeedsInitialization : t.usbIssue[disk.issue ?? "noIdentity"]}</small></span>
                      <span className="drive-badge">{disk.driveLetter ?? t.noDriveLetter}</span>
                      <input type="radio" name="usbDisk" value={usbKey(disk)} disabled={!disk.eligible} checked={usbDiskId === usbKey(disk)}
                        onChange={() => { setUsbDiskId(usbKey(disk)); setReinitializeUsb(false); }} />
                    </label>)}
                  </div>
                  {selectedUsb?.prepared && <label className="usb-reinitialize-option">
                    <input type="checkbox" checked={reinitializeUsb} onChange={event => setReinitializeUsb(event.target.checked)} />
                    <span><strong>{t.usbReinitialize}</strong><small>{t.usbReinitializeWarning}</small></span>
                  </label>}
                  <button type="button" onClick={() => void refreshUsb()}>{t.usbRefresh}</button>
                </> : null}
              </section>
            </div>}
          </fieldset>
          {step === 3 && <section className="operation-screen">
            <div className="operation-intro">
              <div className="operation-emblem" aria-hidden="true"><svg viewBox="0 0 48 48" fill="none">
                <path d="M8 16h32v22H8zM14 9h20v7H14z" stroke="currentColor" strokeWidth="2.5" strokeLinejoin="round" />
                <path d="M17 27h14M21 32h6" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
              </svg></div>
              <div><h2>{output === "usb" ? t.usbWritingTitle : t.isoWritingTitle}</h2>
                <p>{output === "usb" ? t.usbWritingDescription : t.isoWritingDescription}</p></div>
            </div>
            {output === "usb" && selectedUsb && <div className="operation-target">
              <ChoiceIcon kind="usb" /><span><strong>{selectedUsb.friendlyName}</strong>
                <small>{t.diskNumber} {selectedUsb.diskNumber} · {Math.round(selectedUsb.sizeBytes / 1_000_000_000)} GB · {selectedUsb.driveLetter ?? t.noDriveLetter}</small></span>
            </div>}
            <div className="operation-progress" role="status" aria-live="polite">
              <div className="operation-progress-heading"><strong>{output === "usb"
                ? t.usbProgress[usbProgress?.phase ?? "checkingRelease"] : t.usbProgress[usbProgress?.phase ?? "configuringIso"]}</strong>
                <span>{t.usbProgressEstimate} {usbProgress?.percent ?? 0}%</span></div>
              {output === "usb" ? <progress value={usbProgress?.percent ?? 0} max="100" aria-label={t.usbWriteProgress} />
                : <progress value={usbProgress?.percent ?? 0} max="100" aria-label={t.isoCreating} />}
            </div>
            {output === "usb" && <ol className="operation-stages">
              {t.writeStages.map((label, index) => <li key={label} className={index < writeStage ? "done" : index === writeStage ? "active" : ""}>
                <span aria-hidden="true">{index < writeStage ? "✓" : index + 1}</span>{label}</li>)}
            </ol>}
            {pending && <p className="operation-note">{output === "usb" ? t.usbProgressNote : t.isoCreatingNote}</p>}
            {error && <div className="operation-failure" role="alert"><strong>{t.operationFailed}</strong><p>{error}</p></div>}
          </section>}
          {step === 4 && <section className="completion-screen">
            <div className="completion-emblem" aria-hidden="true"><svg viewBox="0 0 48 48" fill="none">
              <circle cx="24" cy="24" r="20" stroke="currentColor" strokeWidth="2.5" />
              <path d="m14 24 7 7 14-15" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" />
            </svg></div>
            <h2>{output === "usb" ? t.usbCompleteTitle : t.isoCompleteTitle}</h2>
            <p className="completion-message" role="status">{message}</p>
            {error && <p className="error" role="alert">{error}</p>}
            <div className="completion-details"><Summary />
              {output === "usb" && selectedUsb && <div className="completion-target"><ChoiceIcon kind="usb" />
                <span><strong>{selectedUsb.friendlyName}</strong>
                  <small>{t.diskNumber} {selectedUsb.diskNumber} · {Math.round(selectedUsb.sizeBytes / 1_000_000_000)} GB · {selectedUsb.driveLetter ?? t.noDriveLetter}</small></span></div>}
              {output === "usb" && sourceIsoVersion && <div className="completion-release">
                <span>{t.sourceIsoVersion}</span><strong>{sourceIsoVersion}</strong>
              </div>}
            </div>
          </section>}
          {step <= 2 && error && <p className="error" role="alert">{error}</p>}
        </main>
        {step < 2 && <aside><Summary /></aside>}
      </div>
      <footer>
        {step <= 2 && <>
          {step > 0 && <button type="button" disabled={pending}
            onClick={() => go(step === 2 ? 1 : 0)}>{t.back}</button>}
          <button type="submit" className="primary" disabled={pending}>
            {pending ? t.saving : step < 2 ? t.next : output === "iso" ? t.saveIso : t.saveUsb}
          </button>
        </>}
        {step === 3 && (pending ? <span className="footer-status">{t.operationInProgress}</span>
          : <button type="button" onClick={() => go(2)}>{t.backToReview}</button>)}
        {step === 4 && <button type="button" className="primary" onClick={() => {
          void invoke("exit_builder").catch(failure => setError(String(failure)));
        }}>{t.exit}</button>}
      </footer>
    </form>
  </>;

  function Summary() {
    return <>
      <h2>{t.summary}</h2>
      <dl>
        <dt>{t.steps[0]}</dt><dd>{sourceSummary}</dd>
        <dt>{t.steps[1]}</dt><dd>{t.usb} · NTFS</dd>
        <dt>{t.backupVerification}</dt><dd>{options.verifyAfterWrite ? t.verifyOn : t.verifyOff}</dd>
      </dl>
    </>;
  }
}

createRoot(document.getElementById("root")!).render(<App />);
