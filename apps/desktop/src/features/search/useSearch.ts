import { useEffect, useRef, useState } from "react";
import { api, asCommandError } from "../../lib/api";
import type { SearchResults } from "../../lib/types";

/** Messages fetched per search; the header says "N+" when a search fills it. */
export const SEARCH_LIMIT = 300;

/**
 * Runs the committed search (Enter, a filter pick, an example). Stale
 * responses are dropped by sequence number, and the previous results stay on
 * screen until the new ones land, so nothing flashes empty. `version` bumps on
 * `index-changed` to re-run the same query.
 */
export function useSearch(query: string, version: number) {
  const [results, setResults] = useState<SearchResults | null>(null);
  const [resultsFor, setResultsFor] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);

  useEffect(() => {
    const my = ++seq.current;
    const q = query.trim();
    if (!q) {
      setResults(null);
      setResultsFor(null);
      setError(null);
      return;
    }
    api.search(q, SEARCH_LIMIT).then(
      (r) => {
        if (my !== seq.current) return;
        setResults(r);
        setResultsFor(q);
        setError(null);
      },
      (e) => {
        if (my === seq.current) setError(asCommandError(e).message);
      },
    );
  }, [query, version]);

  /** True while the shown results belong to an earlier query. */
  const pending = !!query.trim() && resultsFor !== query.trim();
  return { results, error, pending };
}
