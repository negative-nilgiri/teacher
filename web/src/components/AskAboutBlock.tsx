import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { formatReference, type LessonContext, type LineSelection } from "../reference";
import { selectedLines } from "../selection";
import type { LessonNode } from "../types";

interface AskAboutBlockProps {
  lesson: LessonContext;
  node: LessonNode;
}

interface ChipPosition {
  selection: LineSelection;
  top: number;
  left: number;
}

type CopyState = "idle" | "copied" | "manual";

/**
 * A discreet way to ask an agent about a block: an icon that shows on hover or
 * focus, a chip for selected lines, and a popover that copies a reference.
 * Nothing is sent anywhere; the reference only goes to the clipboard.
 */
export function AskAboutBlock({ lesson, node }: AskAboutBlockProps) {
  const anchorRef = useRef<HTMLSpanElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const fieldRef = useRef<HTMLTextAreaElement>(null);
  const popoverId = useId();
  const [open, setOpen] = useState(false);
  const [question, setQuestion] = useState("");
  const [selection, setSelection] = useState<LineSelection | undefined>();
  const [chip, setChip] = useState<ChipPosition | null>(null);
  const [copy, setCopy] = useState<CopyState>("idle");

  const block = () => anchorRef.current?.closest<HTMLElement>(".lesson-block") ?? null;

  useEffect(() => {
    if (node.type !== "code" && node.type !== "diff") return undefined;
    const update = () => {
      const element = block();
      if (!element) return;
      const lines = selectedLines(element, node);
      if (!lines) {
        setChip(null);
        return;
      }
      const range = window.getSelection()?.getRangeAt(0);
      const rect =
        range && typeof range.getBoundingClientRect === "function"
          ? range.getBoundingClientRect()
          : undefined;
      const blockRect = element.getBoundingClientRect();
      setChip({
        selection: lines,
        top: (rect?.bottom ?? blockRect.top) - blockRect.top + 4,
        left: Math.max((rect?.right ?? blockRect.left) - blockRect.left - 40, 8),
      });
    };
    document.addEventListener("selectionchange", update);
    return () => document.removeEventListener("selectionchange", update);
  }, [node]);

  useEffect(() => {
    if (open && copy !== "manual") fieldRef.current?.focus();
  }, [open, copy]);

  useEffect(() => {
    if (copy !== "copied") return undefined;
    const timer = window.setTimeout(() => setCopy("idle"), 2000);
    return () => window.clearTimeout(timer);
  }, [copy]);

  const reference = formatReference(lesson, node, selection, question);

  const openWith = (lines?: LineSelection) => {
    setSelection(lines);
    setChip(null);
    setCopy("idle");
    setOpen(true);
  };

  const close = () => {
    setOpen(false);
    setQuestion("");
    setSelection(undefined);
    triggerRef.current?.focus();
  };

  const copyReference = async () => {
    try {
      if (!navigator.clipboard) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(reference);
      close();
      setCopy("copied");
    } catch {
      setCopy("manual");
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    } else if (event.key === "Enter" && !event.shiftKey && event.target === fieldRef.current) {
      event.preventDefault();
      void copyReference();
    }
  };

  return (
    <span className="ask-anchor" ref={anchorRef}>
      {copy === "copied" ? (
        <span className="ask-copied" role="status">
          Copied
        </span>
      ) : null}
      <button
        aria-controls={open ? popoverId : undefined}
        aria-expanded={open}
        aria-label={`Ask about block ${node.source_id}`}
        className="lesson-block-ask"
        onClick={() => (open ? close() : openWith())}
        ref={triggerRef}
        title="Copy a reference to this block for your agent"
        type="button"
      >
        <svg aria-hidden="true" focusable="false" height="14" viewBox="0 0 16 16" width="14">
          <path
            d="M3 3h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H7l-3 3v-3H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"
            fill="none"
            stroke="currentColor"
            strokeLinejoin="round"
            strokeWidth="1.4"
          />
        </svg>
      </button>
      {chip ? (
        <button
          className="ask-chip"
          onClick={() => openWith(chip.selection)}
          // Keep the text selection while the chip is pressed.
          onMouseDown={(event) => event.preventDefault()}
          style={{ left: chip.left, top: chip.top }}
          type="button"
        >
          Ask
        </button>
      ) : null}
      {open ? (
        <div
          aria-label={`Ask about block ${node.source_id}`}
          className="ask-popover"
          id={popoverId}
          onKeyDown={onKeyDown}
          role="dialog"
        >
          {copy === "manual" ? (
            <>
              <p className="ask-hint">Copying was blocked; the reference is selected, press Ctrl+C or ⌘C.</p>
              <textarea
                aria-label="Reference to copy"
                className="ask-field"
                readOnly
                ref={(element) => element?.select()}
                rows={6}
                value={reference}
              />
            </>
          ) : (
            <>
              <label className="ask-label">
                Question <span className="ask-optional">(optional)</span>
                <textarea
                  className="ask-field"
                  onChange={(event) => setQuestion(event.target.value)}
                  ref={fieldRef}
                  rows={3}
                  value={question}
                />
              </label>
              {selection ? (
                <p className="ask-hint">Includes your selected lines.</p>
              ) : null}
              <button className="ask-copy" onClick={() => void copyReference()} type="button">
                Copy reference
              </button>
            </>
          )}
        </div>
      ) : null}
    </span>
  );
}
