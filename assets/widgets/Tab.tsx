// One tab of a Tabs; the Tabs shows its content when it is open.
export default function Tab({ children }: { title: string; children?: any }) {
  return <div className="tab">{children}</div>;
}
