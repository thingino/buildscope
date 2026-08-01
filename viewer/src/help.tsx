/**
 * Opt-in help balloons, the same shape webflash and the image builder use:
 * `data-help="<i18n key>"` on anything worth explaining, off by default, and
 * remembered under the shared `thingino_help` key so the family behaves alike.
 *
 * Hover is tracked with elementFromPoint rather than per-element listeners.
 * That respects z-order, so a control inside the settings overlay wins over
 * whatever is behind it, and it resolves disabled buttons, which fire no
 * pointer events of their own but are exactly the ones a reader asks about.
 */
import { createContext, useContext, useEffect, useState } from "react";
import { useT } from "./i18n";

const STORAGE_KEY = "thingino_help";

const HelpContext = createContext<{ on: boolean; setOn: (v: boolean) => void }>({
  on: false,
  setOn: () => {},
});

export function useHelp() {
  return useContext(HelpContext);
}

export function HelpProvider({ children }: { children: React.ReactNode }) {
  const [on, setOnState] = useState(() => {
    try {
      return localStorage.getItem(STORAGE_KEY) === "1";
    } catch {
      return false; // storage can be denied; help is not worth failing over
    }
  });

  const setOn = (v: boolean) => {
    setOnState(v);
    try {
      localStorage.setItem(STORAGE_KEY, v ? "1" : "0");
    } catch {
      /* remembering is a nicety */
    }
  };

  // The cursor says which things have something to say, without a hover.
  useEffect(() => {
    document.body.classList.toggle("help-on", on);
    return () => document.body.classList.remove("help-on");
  }, [on]);

  return (
    <HelpContext.Provider value={{ on, setOn }}>
      {children}
      {on && <HelpBalloon />}
    </HelpContext.Provider>
  );
}

function HelpBalloon() {
  const t = useT();
  const [state, setState] = useState<{ key: string; top: number; left: number; above: boolean } | null>(
    null
  );

  useEffect(() => {
    // Measured after paint, so the balloon can be flipped above the target
    // when it would otherwise run off the bottom.
    let raf = 0;
    const onMove = (e: MouseEvent) => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        const under = document.elementFromPoint(e.clientX, e.clientY);
        const el = under instanceof Element ? under.closest("[data-help]") : null;
        const key = el?.getAttribute("data-help");
        if (!el || !key) {
          setState(null);
          return;
        }
        const r = el.getBoundingClientRect();
        setState((prev) =>
          prev?.key === key && prev.top === r.bottom ? prev : { key, top: r.bottom, left: r.left, above: false }
        );
      });
    };
    window.addEventListener("mousemove", onMove);
    return () => {
      window.removeEventListener("mousemove", onMove);
      cancelAnimationFrame(raf);
    };
  }, []);

  if (!state) return null;
  const text = t(state.key);
  // An unresolved key means help was asked for and there is none; say nothing
  // rather than showing the key itself.
  if (!text || text === state.key) return null;

  return (
    <div
      className="help-balloon show"
      role="tooltip"
      style={{ top: state.top + 9, left: state.left, maxWidth: 280 }}
      ref={(node) => {
        if (!node) return;
        // Keep it on screen: clamp horizontally, flip above if it would spill.
        const b = node.getBoundingClientRect();
        const left = Math.min(Math.max(8, state.left), window.innerWidth - b.width - 8);
        if (Math.round(left) !== Math.round(b.left)) node.style.left = `${left}px`;
        if (b.bottom > window.innerHeight - 8) {
          node.style.top = `${Math.max(8, state.top - b.height - 18)}px`;
          node.classList.add("above");
        } else {
          node.classList.remove("above");
        }
      }}
    >
      {text}
    </div>
  );
}
