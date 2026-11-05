import { createContext, useContext } from "react";
import type { Look } from "./theme";

/** Which look is drawing: shared parts (the plan) word and lay themselves out for it. */
export const LookContext = createContext<Look>("proof");

/** The Canvas look: plain labels and explanations instead of the proof's devices. */
export const usePlain = () => useContext(LookContext) === "canvas";
