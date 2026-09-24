import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import {
  asProblem,
  chooseFolder,
  convertPart,
  defaultOutputFolder,
  lookUpPart,
  openFolder,
  planDestination,
  type ConvertRequest,
  type Destination,
  type LibraryMode,
  type PartSummary,
  type Problem,
  type Report,
} from "./bridge.ts";
import { edit, emptyLibraryName, suggest, type LibraryName } from "./library-name.ts";
import { compactPath } from "./path-text.ts";

/** How long typing must pause before the part is looked up. */
const LOOKUP_DELAY_MS = 500;

type Lookup =
  | { state: "empty" }
  | { state: "incomplete" }
  | { state: "looking"; part: string }
  | { state: "found"; summary: PartSummary }
  | { state: "failed"; part: string; problem: Problem };

type Conversion =
  | { state: "idle" }
  | { state: "converting" }
  | { state: "done"; report: Report }
  | { state: "failed"; problem: Problem };

/** A part number is read trimmed and in upper case, like `C25804`. */
function normalizePart(text: string): string | null {
  const part = text.trim().toUpperCase();
  return /^C\d+$/.test(part) ? part : null;
}

export function App() {
  const [defaultFolder, setDefaultFolder] = useState("");
  const [partText, setPartText] = useState("");
  const [lookup, setLookup] = useState<Lookup>({ state: "empty" });
  const [outputFolder, setOutputFolder] = useState("");
  const [mode, setMode] = useState<LibraryMode>("singlePart");
  const [name, setName] = useState<LibraryName>(emptyLibraryName);
  const [symbol, setSymbol] = useState(true);
  const [footprint, setFootprint] = useState(true);
  const [model, setModel] = useState(true);
  const [overwrite, setOverwrite] = useState(false);
  const [projectRelative, setProjectRelative] = useState(true);
  const [destination, setDestination] = useState<Destination | null>(null);
  const [planProblem, setPlanProblem] = useState<Problem | null>(null);
  const [conversion, setConversion] = useState<Conversion>({ state: "idle" });
  const [browsing, setBrowsing] = useState(false);
  const lookupRun = useRef(0);

  const part = normalizePart(partText);
  // The part's own suggestion, or its number when EasyEDA could not be
  // reached to ask. A part EasyEDA does not have gets no name.
  const suggestion =
    lookup.state === "found" && lookup.summary.lcscId === part
      ? lookup.summary.suggestedLibraryName
      : lookup.state === "failed" &&
          lookup.part === part &&
          lookup.problem.code !== "part-not-found"
        ? part
        : "";

  useEffect(() => {
    void defaultOutputFolder().then(setDefaultFolder);
  }, []);

  // Look the part up once typing pauses. A newer lookup makes an older
  // answer irrelevant, so only the latest one lands.
  useEffect(() => {
    const run = ++lookupRun.current;
    if (partText.trim() === "") {
      setLookup({ state: "empty" });
      return;
    }
    if (part === null) {
      setLookup({ state: "incomplete" });
      return;
    }
    setLookup({ state: "looking", part });
    const timer = window.setTimeout(() => {
      lookUpPart(part)
        .then((summary) => {
          if (run === lookupRun.current) setLookup({ state: "found", summary });
        })
        .catch((error: unknown) => {
          if (run === lookupRun.current) {
            setLookup({ state: "failed", part, problem: asProblem(error) });
          }
        });
    }, LOOKUP_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [partText, part]);

  // The library name follows the part as its layout describes.
  useEffect(() => {
    if (part !== null) {
      setName((current) => suggest(current, mode, part, suggestion));
    }
  }, [part, mode, suggestion]);

  const request: ConvertRequest = useMemo(
    () => ({
      lcscId: part ?? "",
      outputFolder,
      mode,
      libraryName: name.value,
      symbol,
      footprint,
      model,
      overwrite,
      projectRelative,
    }),
    [part, outputFolder, mode, name.value, symbol, footprint, model, overwrite, projectRelative],
  );

  // A change to the form makes an earlier result describe something else.
  useEffect(() => {
    setConversion((current) => (current.state === "converting" ? current : { state: "idle" }));
  }, [request]);

  useEffect(() => {
    let current = true;
    if (request.libraryName.trim() === "") {
      setDestination(null);
      setPlanProblem(null);
      return;
    }
    planDestination(request)
      .then((planned) => {
        if (!current) return;
        setDestination(planned);
        setPlanProblem(null);
      })
      .catch((error: unknown) => {
        if (!current) return;
        setDestination(null);
        setPlanProblem(asProblem(error));
      });
    return () => {
      current = false;
    };
  }, [request]);

  const blocker = (() => {
    if (browsing) return "Choose a folder in the folder picker.";
    if (part === null) return "Enter an LCSC part number to convert.";
    if (lookup.state === "failed" && lookup.problem.code === "part-not-found") {
      return "Check the part number above.";
    }
    if (!(symbol || footprint || model)) {
      return "Choose at least one of Symbol, Footprint, or 3D model.";
    }
    // A Single Part Folder name comes from the part, so it waits for the
    // part's details rather than borrow the last part's name.
    if (mode === "singlePart" && (lookup.state === "looking" || name.part !== part)) {
      return "Waiting for the part's details.";
    }
    if (name.value.trim() === "") return "Enter a library name.";
    if (planProblem) return planProblem.text;
    return null;
  })();
  const converting = conversion.state === "converting";

  // Convert waits for the picker, so a folder chosen late never changes the
  // form under a conversion.
  async function browse() {
    setBrowsing(true);
    try {
      const chosen = await chooseFolder(outputFolder || defaultFolder);
      if (chosen !== null) setOutputFolder(chosen);
    } finally {
      setBrowsing(false);
    }
  }

  async function convert() {
    if (blocker !== null || converting) return;
    setConversion({ state: "converting" });
    try {
      setConversion({ state: "done", report: await convertPart(request) });
    } catch (error) {
      setConversion({ state: "failed", problem: asProblem(error) });
    }
  }

  return (
    <form
      className="window"
      onSubmit={(event) => {
        event.preventDefault();
        void convert();
      }}
    >
      <div className="scroll">
        {/* The form holds still while a conversion runs, so its result
            always describes what is on screen. */}
        <fieldset className="content" disabled={converting}>
          <section className="section" aria-labelledby="part-heading">
            <h2 id="part-heading" className="section-title">
              Part
            </h2>
            <label className="field">
              <span className="label">LCSC part number</span>
              <input
                className="input part-input"
                value={partText}
                onChange={(event) => setPartText(event.target.value)}
                placeholder="C25804"
                spellCheck={false}
                autoComplete="off"
                autoFocus
              />
            </label>
            <PartStatus lookup={lookup} />
          </section>

          <section className="section" aria-labelledby="destination-heading">
            <h2 id="destination-heading" className="section-title">
              Destination
            </h2>
            <label className="field">
              <span className="label">Output folder</span>
              <span className="row">
                <input
                  className="input"
                  value={outputFolder}
                  onChange={(event) => setOutputFolder(event.target.value)}
                  placeholder={defaultFolder}
                  spellCheck={false}
                  autoComplete="off"
                />
                <button type="button" className="button" onClick={() => void browse()}>
                  Browse
                </button>
              </span>
              <span className="hint">Leave it empty to use the folder shown.</span>
            </label>

            <fieldset className="field choices">
              <legend className="label">Library layout</legend>
              <label className="choice">
                <input
                  type="radio"
                  name="mode"
                  checked={mode === "singlePart"}
                  onChange={() => setMode("singlePart")}
                />
                <span>
                  <span className="choice-name">Single Part Folder</span>
                  <span className="hint">Each part gets a folder of its own, named after it.</span>
                </span>
              </label>
              <label className="choice">
                <input
                  type="radio"
                  name="mode"
                  checked={mode === "customLibrary"}
                  onChange={() => setMode("customLibrary")}
                />
                <span>
                  <span className="choice-name">Custom Library</span>
                  <span className="hint">
                    One library you name. Parts converted under the same name collect in it.
                  </span>
                </span>
              </label>
            </fieldset>

            <label className="field">
              <span className="label">Library name</span>
              <input
                className="input"
                value={name.value}
                onChange={(event) => setName(edit(event.target.value, part ?? "", suggestion))}
                placeholder={mode === "singlePart" ? "Filled in from the part" : "My_Parts"}
                spellCheck={false}
                autoComplete="off"
              />
            </label>

            {destination && (
              <div className="destination">
                <span className="label">Files go in</span>
                <PathText path={destination.folder} />
                {footprint && (
                  <>
                    <span className="label">The footprint looks for its 3D model in</span>
                    <PathText path={destination.modelReference} />
                  </>
                )}
              </div>
            )}
          </section>

          <section className="section" aria-labelledby="generate-heading">
            <h2 id="generate-heading" className="section-title">
              Generate
            </h2>
            <div className="checks">
              <Check label="Symbol" checked={symbol} onChange={setSymbol} />
              <Check label="Footprint" checked={footprint} onChange={setFootprint} />
              <Check label="3D model" checked={model} onChange={setModel} />
            </div>
            <div className="options">
              <Check
                label="Overwrite"
                hint="Replace a symbol, footprint, or 3D model the library already has."
                checked={overwrite}
                onChange={setOverwrite}
              />
              <Check
                label="Project relative"
                hint={
                  outputFolder.trim() === ""
                    ? "Applies once you choose an output folder, which is then taken as the KiCad project folder."
                    : "The footprint finds its 3D model through ${KIPRJMOD}, taking the output folder as the KiCad project folder."
                }
                checked={projectRelative}
                onChange={setProjectRelative}
              />
            </div>
          </section>
        </fieldset>
      </div>

      <div className="bar">
        <div className="bar-content">
          <button
            type="submit"
            className="button primary convert"
            disabled={blocker !== null || converting}
            aria-busy={converting}
          >
            {converting && <span className="spinner" aria-hidden="true" />}
            {converting ? "Converting" : "Convert"}
          </button>
          <Outcome conversion={conversion} blocker={blocker} />
        </div>
      </div>
    </form>
  );
}

/** A path on a line of its own, shortened from the middle to fit it. */
function PathText({ path }: { path: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [shown, setShown] = useState(path);

  useLayoutEffect(() => {
    const element = ref.current;
    if (element === null) return;
    const context = document.createElement("canvas").getContext("2d");
    const fit = () => {
      if (context === null) {
        setShown(path);
        return;
      }
      context.font = getComputedStyle(element).font;
      const width = element.clientWidth;
      setShown(compactPath(path, (text) => context.measureText(text).width <= width));
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(element);
    return () => observer.disconnect();
  }, [path]);

  return (
    <span ref={ref} className="path" title={path} aria-label={path}>
      {shown}
    </span>
  );
}

function PartStatus({ lookup }: { lookup: Lookup }) {
  switch (lookup.state) {
    case "empty":
      return <p className="status">It starts with C and is on the part's LCSC and JLCPCB pages.</p>;
    case "incomplete":
      return (
        <p className="status">A part number is the letter C followed by digits, like C25804.</p>
      );
    case "looking":
      return <p className="status">Looking up {lookup.part}…</p>;
    case "found": {
      const { summary } = lookup;
      const facts = [summary.manufacturer, summary.package, summary.partClass].filter(Boolean);
      return (
        <div className="status found">
          <span className="part-title">{summary.title}</span>
          {facts.length > 0 && <span className="part-facts">{facts.join(" · ")}</span>}
        </div>
      );
    }
    case "failed":
      return (
        <div className="status problem" role="alert">
          <span>{lookup.problem.text}</span>
          {lookup.problem.detail && <span className="detail">{lookup.problem.detail}</span>}
        </div>
      );
  }
}

function Outcome({ conversion, blocker }: { conversion: Conversion; blocker: string | null }) {
  switch (conversion.state) {
    case "idle":
      return <p className="outcome quiet">{blocker ?? " "}</p>;
    case "converting":
      return <p className="outcome quiet">Fetching the part and writing its files.</p>;
    case "failed":
      return (
        <div className="outcome problem" role="alert">
          <span>{conversion.problem.text}</span>
          {conversion.problem.detail && <span className="detail">{conversion.problem.detail}</span>}
        </div>
      );
    case "done":
      return <Done report={conversion.report} />;
  }
}

function Done({ report }: { report: Report }) {
  const [openProblem, setOpenProblem] = useState<Problem | null>(null);
  const count = report.written.length;
  return (
    <div className="outcome done" role="status">
      <div className="done-row">
        <span className="done-title">
          Saved {count} {count === 1 ? "file" : "files"}.
        </span>
        <button
          type="button"
          className="button"
          onClick={() =>
            void openFolder(report.destination.folder)
              .then(() => setOpenProblem(null))
              .catch((error: unknown) => setOpenProblem(asProblem(error)))
          }
        >
          Open folder
        </button>
      </div>
      {report.notes.map((note) => (
        <span key={note} className="note">
          {note}
        </span>
      ))}
      {openProblem && <span className="problem-inline">{openProblem.text}</span>}
    </div>
  );
}

function Check({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="check">
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span>
        <span className="choice-name">{label}</span>
        {hint && <span className="hint">{hint}</span>}
      </span>
    </label>
  );
}
