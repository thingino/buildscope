// The type checker already carries most of the load here: strict mode is on,
// as are noUnusedLocals and noUnusedParameters. What it cannot see is the
// rule that actually bites in React -- a hook whose dependency list does not
// match what its body closes over -- so that is the reason this exists, and
// the reason it is an error rather than a warning.
import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/**", "node_modules/**", "public/**"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      globals: globals.browser,
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-hooks/exhaustive-deps": "error",
      // A report is JSON from outside this program, so it arrives as unknown
      // and is narrowed at the edges. Casting through it is how that is done.
      "@typescript-eslint/no-explicit-any": "off",
      // Off deliberately, after reviewing all three sites it fires on. Each is
      // the case the rule's own documentation exempts: synchronising with an
      // external system -- reading the inlined report, probing for the API,
      // fetching the report for the build that was just selected. Avoiding the
      // synchronous reset before those loads would mean a data-fetching
      // library or useSyncExternalStore, which is more machinery than three
      // loads justify.
      "react-hooks/set-state-in-effect": "off",
    },
  },
  {
    // The check scripts are Node programs that drive a browser, so the bodies
    // they hand to page.evaluate() are page code living in a Node file: both
    // sets of globals are legitimately in scope here.
    files: ["scripts/**/*.mjs"],
    languageOptions: { globals: { ...globals.node, ...globals.browser } },
  }
);
