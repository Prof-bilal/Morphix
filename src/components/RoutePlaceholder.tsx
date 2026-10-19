import type { ReactNode } from "react";

interface RoutePlaceholderProps {
  title: string;
  /** What this screen will show once its phase lands, in the user's terms. */
  lead: string;
  /** Which task fills this screen in. Kept honest: no screen claims working behavior. */
  note: string;
  /** Extra content (for example the mounted route parameters). */
  children?: ReactNode;
}

/**
 * Phase 0 stand-in for a screen. It states the planned purpose and names the task that
 * implements it, so the shell never implies functionality that does not exist yet
 * (UI_SPEC.md §2).
 */
export function RoutePlaceholder({ title, lead, note, children }: RoutePlaceholderProps) {
  return (
    <section className="page" aria-labelledby="page-title">
      <h1 className="page__title" id="page-title">
        {title}
      </h1>
      <p className="page__lead">{lead}</p>
      {children}
      <p className="page__note">Planned: {note}</p>
    </section>
  );
}
