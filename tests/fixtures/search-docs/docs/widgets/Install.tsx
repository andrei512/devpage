// The last slide's picture: the two commands, big.
import { Window, framesCss } from "./frames";

export const css = framesCss + `
  .inst-cmd { font-family: var(--font-mono); font-size: 52px; line-height: 1.9; white-space: nowrap; }
  .inst-cmd .p { color: #98989d; }
`;

export default function Install() {
  return (
    <Window title="my-app" dark style={{ width: "1240px" }}>
      <div className="inst-cmd">
        <div><span className="p">$ </span>cargo install devpage --locked</div>
        <div><span className="p">$ </span>devpage docs</div>
      </div>
    </Window>
  );
}
