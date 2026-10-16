import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { formatReference, selectionLabel, type LessonContext, type LineSelection } from "./reference";
import type { LessonNode } from "./types";

/** One question to ask, kept as data so it can still be edited. */
export interface AskEntry {
  id: number;
  /** `source_id` of the block asked about. */
  block: string;
  selection?: LineSelection;
  question: string;
}

interface AskList {
  entries: AskEntry[];
  add: (entry: Omit<AskEntry, "id">) => void;
  update: (id: number, question: string) => void;
  remove: (id: number) => void;
  clear: () => void;
}

const AskListContext = createContext<AskList | null>(null);

/** The list, or `null` where no provider is mounted. */
export const useAskList = () => useContext(AskListContext);

// The list survives a refresh but not a new `learn` run, which is enough for
// questions that are pasted to an agent and then discarded.
function load(key: string): AskEntry[] {
  try {
    const parsed: unknown = JSON.parse(window.sessionStorage.getItem(key) ?? "[]");
    return Array.isArray(parsed) ? (parsed as AskEntry[]) : [];
  } catch {
    return [];
  }
}

export function AskListProvider({ lesson, children }: { lesson: LessonContext; children: ReactNode }) {
  const key = `learn-asks:${lesson.artifact_path ?? lesson.title}`;
  const [entries, setEntries] = useState(() => load(key));

  useEffect(() => {
    try {
      window.sessionStorage.setItem(key, JSON.stringify(entries));
    } catch {
      // Storage can be unavailable; the list then lasts until the page closes.
    }
  }, [key, entries]);

  const add = useCallback(
    (entry: Omit<AskEntry, "id">) =>
      setEntries((current) => [...current, { ...entry, id: Math.max(0, ...current.map((e) => e.id)) + 1 }]),
    [],
  );
  const update = useCallback(
    (id: number, question: string) =>
      setEntries((current) => current.map((e) => (e.id === id ? { ...e, question } : e))),
    [],
  );
  const remove = useCallback((id: number) => setEntries((current) => current.filter((e) => e.id !== id)), []);
  const clear = useCallback(() => setEntries([]), []);
  const value = useMemo(() => ({ entries, add, update, remove, clear }), [entries, add, update, remove, clear]);

  return <AskListContext.Provider value={value}>{children}</AskListContext.Provider>;
}

/** Every entry as the reference Ask copies for one block, numbered. */
export function formatAll(lesson: LessonContext, nodes: LessonNode[], entries: AskEntry[]): string {
  const found = entries.flatMap((entry) => {
    const node = nodes.find((candidate) => candidate.source_id === entry.block);
    return node ? [{ entry, node }] : [];
  });
  return found
    .map(({ entry, node }, index) => `[${index + 1}/${found.length}] ${formatReference(lesson, node, entry.selection, entry.question)}`)
    .join("\n\n");
}

/** The button and panel for the collected questions; nothing until one is added. */
export function AskListPanel({ lesson, nodes }: { lesson: LessonContext; nodes: LessonNode[] }) {
  const list = useAskList();
  const [open, setOpen] = useState(false);
  const [manual, setManual] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return undefined;
    const timer = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);

  if (!list || list.entries.length === 0) return null;
  const { entries, update, remove, clear } = list;
  const text = formatAll(lesson, nodes, entries);

  const copyAll = async () => {
    try {
      if (!navigator.clipboard) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(text);
      setManual(false);
      setCopied(true);
    } catch {
      setManual(true);
    }
  };

  return (
    <aside className="ask-list" aria-label="Collected questions">
      {open ? (
        <div className="ask-list-panel">
          <ol className="ask-list-entries">
            {entries.map((entry) => {
              const where = [`block ${entry.block}`, selectionLabel(entry.selection)].filter(Boolean).join(", ");
              return (
                <li key={entry.id}>
                  <label className="ask-label">
                    {where}
                    <textarea
                      className="ask-field"
                      onChange={(event) => update(entry.id, event.target.value)}
                      rows={2}
                      value={entry.question}
                    />
                  </label>
                  <button aria-label={`Remove question about ${where}`} onClick={() => remove(entry.id)} type="button">
                    Remove
                  </button>
                </li>
              );
            })}
          </ol>
          {manual ? (
            <>
              <p className="ask-hint">Copying was blocked; the text is selected, press Ctrl+C or ⌘C.</p>
              <textarea
                aria-label="Questions to copy"
                className="ask-field"
                readOnly
                ref={(element) => element?.select()}
                rows={8}
                value={text}
              />
            </>
          ) : null}
          <div className="ask-list-actions">
            <button onClick={clear} type="button">
              Clear
            </button>
            <button className="ask-copy" onClick={() => void copyAll()} type="button">
              Copy all
            </button>
          </div>
        </div>
      ) : null}
      <button aria-expanded={open} className="ask-list-toggle" onClick={() => setOpen(!open)} type="button">
        Asks ({entries.length}){copied ? " · copied" : ""}
      </button>
    </aside>
  );
}
