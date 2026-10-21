import { useRef, useState } from "react";

interface CopyButtonProps {
  /** Exact text placed on the clipboard. */
  text: string;
  /** Short noun for the accessible label, for example "log path". */
  label: string;
}

/**
 * Copies `text` to the clipboard with a visible confirmation. Falls back to a
 * selection-based copy where the async clipboard API is unavailable; a failure leaves
 * the button unchanged so a failed copy never presents as a success.
 */
export function CopyButton({ text, label }: CopyButtonProps) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  async function copy(): Promise<void> {
    try {
      if (navigator.clipboard?.writeText !== undefined) {
        await navigator.clipboard.writeText(text);
      } else {
        const area = document.createElement("textarea");
        area.value = text;
        area.setAttribute("readonly", "");
        document.body.appendChild(area);
        area.select();
        const ok = document.execCommand("copy");
        area.remove();
        if (!ok) {
          return;
        }
      }
      setCopied(true);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  }

  return (
    <button type="button" className="copy-button" onClick={copy} aria-live="polite">
      {copied ? "Copied" : `Copy ${label}`}
    </button>
  );
}
