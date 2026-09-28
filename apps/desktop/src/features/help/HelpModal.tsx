import { Kbd, Modal, Row, Section } from "../../components/Modal";

const FILTERS: { op: string; what: string; example: string }[] = [
  { op: "from:name  from:me", what: "Who sent it", example: "from:jordan game" },
  { op: "in:name  with:name", what: "Which conversation, by group name or person", example: 'in:"Lake House 🏠" kayaks' },
  { op: "has:link  photo  video  audio  file", what: "Messages with a link or an attachment", example: "has:photo from:nora" },
  { op: "before:  after:  during:", what: "Dates as 2024, 2024-06 or 2024-06-15", example: "during:2024 sunset" },
  { op: '"exact phrase"', what: "Words together, in this order", example: '"see you soon"' },
  { op: "-word", what: "Leave out messages with a word", example: "dinner -pizza" },
];

const KEYS: { keys: string[]; what: string }[] = [
  { keys: ["⌘", "K"], what: "Search" },
  { keys: ["Return"], what: "Search, or open the selected result" },
  { keys: ["↑", "↓"], what: "Move through results" },
  { keys: ["→"], what: "Move into the open conversation" },
  { keys: ["Esc"], what: "Close the conversation, or go back" },
  { keys: ["⌘", "O"], what: "Open the conversation in Messages" },
  { keys: ["⌘", "/"], what: "This help" },
  { keys: ["⌘", ","], what: "Settings" },
  { keys: ["⌘", "1", "2", "3"], what: "Search, Insights, Storage" },
  { keys: ["←", "→"], what: "Step through photos in the viewer" },
];

export function HelpModal({ onClose, onRun, onSettings }: { onClose: () => void; onRun: (q: string) => void; onSettings: () => void }) {
  return (
    <Modal title="How to search" onClose={onClose}>
      <Section title="Words and meaning">
        <Row label="Words" detail="Finds messages with all your words. The start of a word is enough, so gat finds gate." />
        <Row
          label="Meaning"
          detail="Also finds messages about the same thing in other words. These are marked Similar."
        >
          <button className="example-btn" onClick={() => onRun("where should we eat in austin")}>
            where should we eat in austin
          </button>
        </Row>
      </Section>

      <Section title="Filters">
        <p className="modal-note">Add these anywhere in the search, or use the filter menu under the search bar. Click an example to run it.</p>
        {FILTERS.map((f) => (
          <Row key={f.op} label={<code className="op">{f.op}</code>} detail={f.what}>
            <button className="example-btn" onClick={() => onRun(f.example)}>
              {f.example}
            </button>
          </Row>
        ))}
      </Section>

      <Section title="Keyboard">
        {KEYS.map((k) => (
          <Row key={k.what} label={k.what}>
            <span className="kbd-row">
              {k.keys.map((key) => (
                <Kbd key={key}>{key}</Kbd>
              ))}
            </span>
          </Row>
        ))}
      </Section>

      <Section title="Privacy">
        <p className="modal-note">
          Messages Search reads your history on this Mac, read-only. The index and the meaning model stay on this Mac, and
          nothing is uploaded.
        </p>
      </Section>

      <div className="modal-foot">
        <span>Highlight style and index details are in Settings.</span>
        <button className="btn" onClick={onSettings}>
          Open Settings
        </button>
      </div>
    </Modal>
  );
}
