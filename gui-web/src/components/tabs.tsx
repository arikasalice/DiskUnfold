import { useId } from "react";
import type { ReactNode } from "react";
import {
  Tab,
  TabList,
  TabPanel,
  Tabs as AriaTabs,
} from "react-aria-components";
import "../theme/tabs.css";

/** One tab of {@link Tabs}. */
export type TabsItem<K extends string> = {
  /** Stable identity of the tab; it is what the selection reports. */
  key: K;
  /** The tab's name. The accessible name is the visible text: the label, then the count. */
  label: string;
  /** A short count drawn after the label and read as part of the tab's name. */
  count?: string;
  /** Extra text for assistive technology only, read as the tab's description. */
  description?: string;
};

/**
 * A row of tabs over one panel. The selected tab is bold with an underline, so
 * the selection never rests on colour alone. Left and Right move between tabs,
 * Home and End jump to the ends, and a focused tab is selected at once.
 *
 * `children` is the panel: it stays mounted while the selection changes and
 * is labelled by the selected tab. The owning feature decides what each key
 * shows. `value` must be the key of one of `items`.
 */
export function Tabs<K extends string>({
  label,
  items,
  value,
  onChange,
  children,
}: {
  /** Accessible name of the tab list. */
  label: string;
  items: readonly TabsItem<K>[];
  value: K;
  onChange: (key: K) => void;
  /** The content of the selected tab's panel. */
  children?: ReactNode;
}) {
  const base = useId();
  return (
    <AriaTabs
      className="du-tabs"
      selectedKey={value}
      onSelectionChange={(key) => {
        const next = items.find((item) => item.key === key);
        if (next) onChange(next.key);
      }}
    >
      <TabList className="du-tabs__list" aria-label={label}>
        {items.map((item) => {
          const descriptionId = `${base}-${item.key}-description`;
          return (
            <Tab
              key={item.key}
              id={item.key}
              {...(item.description !== undefined && {
                "aria-describedby": descriptionId,
              })}
              className="du-tab"
            >
              <span className="du-tab__label">{item.label}</span>
              {item.count !== undefined && (
                <>
                  {/* Keeps the label and the count apart in the computed name. */}{" "}
                  <span className="du-tab__count">{item.count}</span>
                </>
              )}
            </Tab>
          );
        })}
      </TabList>
      {/* Descriptions live outside the tabs so they never join a tab's name. */}
      <div hidden>
        {items.map(
          (item) =>
            item.description !== undefined && (
              <span key={item.key} id={`${base}-${item.key}-description`}>
                {item.description}
              </span>
            ),
        )}
      </div>
      <TabPanel id={value} className="du-tabs__panel">
        {children}
      </TabPanel>
    </AriaTabs>
  );
}
