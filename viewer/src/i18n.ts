// Tiny i18n runtime, dependency-free and CSP-safe (no eval, no external
// fetch), following the same conventions as the other thingino web apps:
// the same 15 languages in the same order, the same native language names,
// browser detection with a localStorage override, and dir="rtl" for
// right-to-left languages.
//
// Differences from the sibling apps, both because this viewer is React
// rather than static markup:
//   * strings are read through `useT()` at render time instead of being
//     stamped onto `data-i18n` elements by an apply() pass
//   * only English is bundled eagerly; other languages load on demand, so
//     the byte cost of 15 dictionaries is not paid by every visitor
//
// What is deliberately NOT translated: anything that comes out of a report.
// Package and partition names, image formats, and the diagnostic warnings
// produced by the analysis core are the same words in every language, and
// the core emits them as finished prose for the CLI as well.

import { useCallback, useContext, useEffect, useMemo, useState } from "react";
import { createContext } from "react";
import en from "./locales/en";

export const SUPPORTED = [
  "en",
  "es",
  "fr",
  "de",
  "it",
  "nl",
  "pl",
  "pt",
  "tr",
  "uk",
  "ru",
  "ar",
  "zh-CN",
  "ja",
  "ko",
] as const;

export type Lang = (typeof SUPPORTED)[number];

export const NAMES: Record<Lang, string> = {
  en: "English",
  es: "Español",
  fr: "Français",
  de: "Deutsch",
  it: "Italiano",
  nl: "Nederlands",
  pl: "Polski",
  pt: "Português",
  tr: "Türkçe",
  uk: "Українська",
  ru: "Русский",
  ar: "العربية",
  "zh-CN": "中文",
  ja: "日本語",
  ko: "한국어",
};

const RTL: readonly string[] = ["ar", "he", "fa", "ur"];

export type Dict = Record<string, string>;

// English is the fallback for every key, so it is always present.
const LOADERS: Record<Lang, () => Promise<Dict>> = {
  en: async () => en,
  es: () => import("./locales/es").then((m) => m.default),
  fr: () => import("./locales/fr").then((m) => m.default),
  de: () => import("./locales/de").then((m) => m.default),
  it: () => import("./locales/it").then((m) => m.default),
  nl: () => import("./locales/nl").then((m) => m.default),
  pl: () => import("./locales/pl").then((m) => m.default),
  pt: () => import("./locales/pt").then((m) => m.default),
  tr: () => import("./locales/tr").then((m) => m.default),
  uk: () => import("./locales/uk").then((m) => m.default),
  ru: () => import("./locales/ru").then((m) => m.default),
  ar: () => import("./locales/ar").then((m) => m.default),
  "zh-CN": () => import("./locales/zh-CN").then((m) => m.default),
  ja: () => import("./locales/ja").then((m) => m.default),
  ko: () => import("./locales/ko").then((m) => m.default),
};

function isSupported(v: string): v is Lang {
  return (SUPPORTED as readonly string[]).includes(v);
}

/**
 * `?lang=` in the URL, else the saved choice, else the first supported browser
 * language, else English. The query parameter makes a link open in a specific
 * language without changing the reader's saved preference.
 */
export function detect(): Lang {
  try {
    const fromUrl = new URLSearchParams(window.location.search).get("lang");
    if (fromUrl && isSupported(fromUrl)) return fromUrl;
  } catch {
    /* malformed query */
  }
  try {
    const saved = localStorage.getItem("lang");
    if (saved && isSupported(saved)) return saved;
  } catch {
    /* private mode */
  }
  const candidates = navigator.languages ?? [navigator.language || "en"];
  for (const c of candidates) {
    if (!c) continue;
    if (isSupported(c)) return c; // exact, e.g. zh-CN
    const base = c.split("-")[0];
    if (base === "zh") return "zh-CN"; // any Chinese to Simplified
    if (isSupported(base)) return base; // es-MX to es, pt-BR to pt
  }
  return "en";
}

/** `{name}` placeholders are replaced from `params`. */
export function interpolate(s: string, params?: Record<string, string | number>): string {
  if (!params) return s;
  let out = s;
  for (const [k, v] of Object.entries(params)) out = out.split(`{${k}}`).join(String(v));
  return out;
}

export type TFn = (key: string, params?: Record<string, string | number>) => string;

export interface I18nValue {
  lang: Lang;
  setLang: (l: Lang) => void;
  t: TFn;
}

export const I18nContext = createContext<I18nValue>({
  lang: "en",
  setLang: () => {},
  t: (key, params) => interpolate(en[key] ?? key, params),
});

export function useI18n(): I18nValue {
  return useContext(I18nContext);
}

/** Most call sites only need the translate function. */
export function useT(): TFn {
  return useContext(I18nContext).t;
}

/** State hook for the provider; App wires the returned value into context. */
export function useI18nState(): I18nValue {
  const [lang, setLangState] = useState<Lang>(() => detect());
  const [dict, setDict] = useState<Dict>(en);

  useEffect(() => {
    let cancelled = false;
    void LOADERS[lang]()
      .then((d) => {
        if (!cancelled) setDict(d);
      })
      .catch(() => {
        if (!cancelled) setDict(en); // a missing dictionary reads as English
      });
    document.documentElement.lang = lang;
    document.documentElement.dir = RTL.includes(lang) ? "rtl" : "ltr";
    return () => {
      cancelled = true;
    };
  }, [lang]);

  const setLang = useCallback((l: Lang) => {
    setLangState(l);
    try {
      localStorage.setItem("lang", l);
    } catch {
      /* private mode */
    }
  }, []);

  const t = useCallback<TFn>(
    (key, params) => interpolate(dict[key] ?? en[key] ?? key, params),
    [dict]
  );

  return useMemo(() => ({ lang, setLang, t }), [lang, setLang, t]);
}
