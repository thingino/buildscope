/**
 * How sizes read: rounded, exact bytes, or hex.
 *
 * Rounding is right for reading a report and wrong for working with one: a
 * rootfs at "5.50 MiB" of "5.50 MiB" is the interesting case precisely because
 * the two differ, and the 140 bytes between them are invisible until the
 * rounding stops. So it is a preference, remembered like the help toggle.
 *
 * The flag lives in `format.ts` as a module value rather than being threaded
 * through as a parameter: `humanBytes` is called from about eighty places, all
 * of them during render, so a re-render of the tree is enough to pick up a
 * change and none of those calls sit inside a memo that could hold a stale
 * string. The provider below owns the React state that causes that re-render,
 * and keeps the module flag in step with it.
 */
import { createContext, useContext, useEffect, useState } from "react";
import { setUnits as apply, Units } from "./format";

const STORAGE_KEY = "buildscope_units";
const CHOICES: Units[] = ["human", "bytes", "hex"];

const UnitsContext = createContext<{ units: Units; setUnits: (v: Units) => void }>({
  units: "human",
  setUnits: () => {},
});

export function useUnits() {
  return useContext(UnitsContext);
}

function stored(): Units {
  try {
    const v = localStorage.getItem(STORAGE_KEY) as Units | null;
    return v && CHOICES.includes(v) ? v : "human";
  } catch {
    return "human"; // storage can be denied; a display preference is not worth failing over
  }
}

export function UnitsProvider({ children }: { children: React.ReactNode }) {
  // Read before the first paint, so a reader who chose exact bytes never sees
  // a frame of rounded ones.
  const [units, setState] = useState<Units>(() => {
    const v = stored();
    apply(v);
    return v;
  });

  const setUnits = (v: Units) => {
    apply(v);
    setState(v);
    try {
      localStorage.setItem(STORAGE_KEY, v);
    } catch {
      /* remembering is a nicety */
    }
  };

  // Guards against the module flag and the state disagreeing after a hot
  // reload, where the module can be replaced while the state survives.
  useEffect(() => {
    apply(units);
  }, [units]);

  return <UnitsContext.Provider value={{ units, setUnits }}>{children}</UnitsContext.Provider>;
}
