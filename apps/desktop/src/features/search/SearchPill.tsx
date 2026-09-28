import { Icon } from "../../components/Icon";

/**
 * The pill search bar with the Search button inside on the right. Searching
 * happens on Enter (or the button), Google style; `onKeyDown` may claim a key
 * first by calling preventDefault (the results page uses Enter to open the
 * selected result when the text hasn't changed).
 */
export function SearchPill({
  value,
  onChange,
  onSubmit,
  inputRef,
  onKeyDown,
  size,
  autoFocus,
  submit = true,
}: {
  value: string;
  onChange: (v: string) => void;
  onSubmit: () => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
  onKeyDown?: (e: React.KeyboardEvent<HTMLInputElement>) => void;
  size: "hero" | "bar";
  autoFocus?: boolean;
  /** Show the attached Search button (home); Enter always searches. */
  submit?: boolean;
}) {
  return (
    <form
      className={`search-pill ${size}`}
      role="search"
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit();
      }}
    >
      <Icon name="search" size={size === "hero" ? 18 : 16} className="search-pill-icon" />
      <input
        ref={inputRef}
        className="search-pill-input"
        type="text"
        value={value}
        placeholder={size === "hero" ? "Search words, people, or what it was about" : "Search"}
        aria-label="Search messages"
        spellCheck={false}
        autoCorrect="off"
        autoCapitalize="off"
        autoComplete="off"
        autoFocus={autoFocus}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
      />
      {value && (
        <button
          type="button"
          className="search-pill-clear"
          aria-label="Clear"
          tabIndex={-1}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => {
            onChange("");
            inputRef.current?.focus();
          }}
        >
          <Icon name="clear" size={16} />
        </button>
      )}
      {submit && (
        <button type="submit" className="btn primary search-pill-submit" onMouseDown={(e) => e.preventDefault()}>
          Search
        </button>
      )}
    </form>
  );
}
