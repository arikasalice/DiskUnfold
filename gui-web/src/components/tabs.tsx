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
  /** The tab's name, which is also its accessible name. */
  label: string;
  /** A short count drawn after the label; assistive technology reads it as the tab's description. */
  count?: string;
  /** Extra text for assistive technology only, read after the count. */
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
          const countId = `${base}-${item.key}-count`;
          const descriptionId = `${base}-${item.key}-description`;
          const described = [
            item.count !== undefined ? countId : "",
            item.description !== undefined ? descriptionId : "",
          ]
            .filter(Boolean)
            .join(" ");
          return (
            <Tab
              key={item.key}
              id={item.key}
              aria-label={item.label}
              {...(described !== "" && { "aria-describedby": described })}
              className="du-tab"
            >
              <span className="du-tab__label">{item.label}</span>
              {item.count !== undefined && (
                <span className="du-tab__count" id={countId}>
                  {item.count}
                </span>
              )}
              {item.description !== undefined && (
                <span className="du-visually-hidden" id={descriptionId}>
                  {item.description}
                </span>
              )}
            </Tab>
          );
        })}
      </TabList>
      <TabPanel id={value} className="du-tabs__panel">
        {children}
      </TabPanel>
    </AriaTabs>
  );
}
