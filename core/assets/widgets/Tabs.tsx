import { useState, Children } from "react";

// Tabs, the first one open. Its children are Tab widgets; each one's title
// labels its tab.
export default function Tabs({ children }: { children?: any }) {
  const tabs = Children.toArray(children).filter((child: any) => child?.props?.title !== undefined);
  const [open, setOpen] = useState(0);
  return (
    <div className="tabs">
      <div className="tabs-bar" role="tablist">
        {tabs.map((tab: any, i: number) => (
          <button role="tab" aria-selected={i === open} className={i === open ? "current" : ""} onClick={() => setOpen(i)}>
            {tab.props.title}
          </button>
        ))}
      </div>
      <div className="tabs-panel" role="tabpanel">{tabs[open]}</div>
    </div>
  );
}
