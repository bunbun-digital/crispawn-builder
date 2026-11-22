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
type PreparedUsbDisk = { diskNumber: number; friendlyName: string; uniqueId: string; sizeBytes: number; driveLetter: string | null };
type InternalDisk = { diskNumber: number; friendlyName: string; sizeBytes: number; busType: string };
type UsbPhase = keyof typeof strings.ja.usbProgress;
type UsbProgress = { phase: UsbPhase; percent: number };

function ChoiceIcon({ kind }: { kind: "disk" | "usb" }) {
  return <span className="choice-icon" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none"
    stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    {kind === "disk" && <><rect x="3" y="5" width="18" height="14" rx="3" /><path d="M7 10h10M7 15h.01M11 15h.01" /></>}
    {kind === "usb" && <><path d="M7 3h10v5H7zM5 8h14v12H5zM9 16h6" /></>}
  </svg></span>;
}

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
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [isoAvailable, setIsoAvailable] = useState(false);
  const [hypervAvailable, setHypervAvailable] = useState(false);
  const [testBoot, setTestBoot] = useState(false);
  const [output, setOutput] = useState("usb");
  const [baseSha256, setBaseSha256] = useState("");
  const [usbDisks, setUsbDisks] = useState<PreparedUsbDisk[]>([]);
  const [usbDiskId, setUsbDiskId] = useState("");
  const [usbProgress, setUsbProgress] = useState<UsbProgress | null>(null);
  const t = strings[locale];
  const selectedUsb = usbDisks.find(disk => disk.uniqueId === usbDiskId);
  const writeStage = (usbProgress?.percent ?? 0) >= 73 ? 2 : (usbProgress?.percent ?? 0) >= 25 ? 1 : 0;
  const sourceIds: string[] = [];
  const sourceSummary = pcMode === "other" ? t.all : internalDisks
    .filter(disk => selectedDiskNumbers.includes(disk.diskNumber))
    .map(disk => disk.friendlyName).join(", ") || t.noSelectedDisks;

  useEffect(() => {
    invoke<boolean>("iso_available").then(setIsoAvailable).catch(() => setIsoAvailable(false));
    invoke<boolean>("hyperv_available").then(setHypervAvailable).catch(() => setHypervAvailable(false));
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
      const disks = await invoke<PreparedUsbDisk[]>("prepared_usb_disks");
      setUsbDisks(disks);
      setUsbDiskId(current => disks.some(disk => disk.uniqueId === current) ? current : (disks[0]?.uniqueId ?? ""));
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
  }

  function go(next: Step) {
    clearResult();
    setStep(next);
  }

  async function submit() {
    clearResult();
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
      startSeconds: Number(start),
      poweroffSeconds: stay ? null : Number(off),
    };
    setPending(true);
    let unlisten: (() => void) | undefined;
    try {
      if (output === "usb") {
        const disk = selectedUsb;
        if (!disk) throw new Error(t.usbSelectDisk);
        setUsbProgress({ phase: "selectingIso", percent: 0 });
        unlisten = await listen<UsbProgress>("usb-write-progress", event => {
          setUsbProgress(event.payload);
          setStep(3);
        });
        if (await invoke<boolean>("save_usb", { disk, settings, pcMode,
          selectedDiskNumbers, uiLanguage: locale })) {
          setUsbProgress({ phase: "complete", percent: 100 });
          setMessage(t.usbSaved);
          setStep(4);
        } else {
          setUsbProgress(null);
          setStep(2);
        }
      } else if (output === "iso") {
        const verifyBoot = testBoot;
        setStep(3);
        if (await invoke<boolean>("save_iso", {
          settings, baseSha256, testBoot: verifyBoot,
        })) {
          setMessage(verifyBoot ? t.bootPassed : t.isoSaved);
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
    <form onSubmit={event => {
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
                <h2>{t.start}</h2>
                <select aria-label={t.start} defaultValue="auto">
                  <option value="auto">{t.auto}</option>
                  <option disabled>{t.menu} · {t.unavailable}</option>
                  <option disabled>{t.restore} · {t.unavailable}</option>
                </select>
                <div className="pair">
                  <label>{t.startDelay}
                    <input type="number" min="0" max="300" required value={start}
                      onChange={event => setStart(event.target.value)} />
                  </label>
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
              </section>
              <section className="output">
                <h2>{t.output}</h2>
                <select aria-label={t.output} value={output} onChange={event => setOutput(event.target.value)}>
                  <option value="iso" disabled={!isoAvailable}>{t.iso}{!isoAvailable && ` · ${t.unavailable}`}</option>
                  <option value="usb">{t.bootUsb}</option>
                </select>
                {output === "iso" ? <>
                  <p>{t.isoHelp}</p>
                  {hypervAvailable && <label className="boot-test"><input type="checkbox"
                    checked={testBoot} onChange={event => setTestBoot(event.target.checked)} />{t.bootTest}</label>}
                  <label>{t.baseDigest}<input required pattern="[0-9a-fA-F]{64}" value={baseSha256}
                    onChange={event => setBaseSha256(event.target.value)} /></label>
                </> : output === "usb" ? <>
                  <h3 className="usb-heading">{t.usbDisk}</h3>
                  <div className="usb-target-list" role="radiogroup" aria-label={t.usbDisk}>
                    {usbDisks.length === 0 && <p>{t.usbNoDisk}</p>}
                    {usbDisks.map(disk => <label key={disk.uniqueId}
                      className={`choice-card usb-card${usbDiskId === disk.uniqueId ? " selected" : ""}`}>
                      <ChoiceIcon kind="usb" />
                      <span className="choice-copy"><strong>{disk.friendlyName}</strong>
                        <small>{t.diskNumber} {disk.diskNumber} · {Math.round(disk.sizeBytes / 1_000_000_000)} GB</small></span>
                      <span className="drive-badge">{disk.driveLetter ?? t.noDriveLetter}</span>
                      <input type="radio" name="usbDisk" value={disk.uniqueId} checked={usbDiskId === disk.uniqueId}
                        onChange={() => setUsbDiskId(disk.uniqueId)} />
                    </label>)}
                  </div>
                  <button type="button" onClick={() => void refreshUsb()}>{t.usbRefresh}</button>
                </> : null}
                {output === "iso" && !isoAvailable && <p>{t.isoTools}</p>}
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
                ? t.usbProgress[usbProgress?.phase ?? "selectingIso"] : t.isoCreating}</strong>
                {output === "usb" && <span>{t.usbProgressEstimate} {usbProgress?.percent ?? 0}%</span>}</div>
              {output === "usb" ? <progress value={usbProgress?.percent ?? 0} max="100" aria-label={t.usbWriteProgress} />
                : <progress aria-label={t.isoCreating} />}
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
            <div className="completion-details"><Summary />
              {output === "usb" && selectedUsb && <div className="completion-target"><ChoiceIcon kind="usb" />
                <span><strong>{selectedUsb.friendlyName}</strong>
                  <small>{t.diskNumber} {selectedUsb.diskNumber} · {Math.round(selectedUsb.sizeBytes / 1_000_000_000)} GB · {selectedUsb.driveLetter ?? t.noDriveLetter}</small></span></div>}
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
        {step === 4 && <button type="button" className="primary" onClick={() => go(0)}>{t.createAnother}</button>}
      </footer>
    </form>
  </>;

  function Summary() {
    return <>
      <h2>{t.summary}</h2>
      <dl>
        <dt>{t.steps[0]}</dt><dd>{sourceSummary}</dd>
        <dt>{t.steps[1]}</dt><dd>{t.usb} · NTFS</dd>
      </dl>
    </>;
  }
}

createRoot(document.getElementById("root")!).render(<App />);
