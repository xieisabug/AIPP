import { createContext } from "react";

// Keep execution widgets out of compact chat even during streaming or renderer fallback.
export const ButlerResultsOnlyContext = createContext(false);
