/**
 * The app's Rust commands, typed. Every call to the native side goes
 * through here.
 */
import { invoke } from "@tauri-apps/api/core";

export type LibraryMode = "singlePart" | "customLibrary";

export interface PartSummary {
  lcscId: string;
  title: string;
  manufacturer: string;
  package: string;
  partClass: string;
  suggestedLibraryName: string;
}

export interface ConvertRequest {
  lcscId: string;
  outputFolder: string;
  mode: LibraryMode;
  libraryName: string;
  symbol: boolean;
  footprint: boolean;
  model: boolean;
  overwrite: boolean;
  projectRelative: boolean;
}

export interface Destination {
  libraryName: string;
  folder: string;
  symbolLibrary: string;
  footprintLibrary: string;
  modelFolder: string;
  modelReference: string;
  projectRelative: boolean;
  /** The KiCad project the libraries are added to, when the folder is in one. */
  projectFile: string | null;
}

export interface Report {
  destination: Destination;
  written: string[];
  /** The project's library tables the libraries were added to. */
  tables: string[];
  /** `library:symbol`, as the project's schematic finds the symbol. */
  symbolId: string | null;
  notes: string[];
}

/** A failure as the native side words it. */
export interface Problem {
  code: string;
  text: string;
  detail: string | null;
}

/** Anything a command rejects with that is not a Problem reads as one. */
export function asProblem(error: unknown): Problem {
  if (typeof error === "object" && error !== null && "code" in error && "text" in error) {
    return error as Problem;
  }
  return {
    code: "unexpected",
    text: "Something went wrong inside the app. Try again, and restart it if this keeps happening.",
    detail: String(error),
  };
}

export const defaultOutputFolder = () => invoke<string>("default_output_folder");

export const lookUpPart = (lcscId: string) => invoke<PartSummary>("look_up_part", { lcscId });

export const planDestination = (request: ConvertRequest) =>
  invoke<Destination>("plan_destination", { request });

export const convertPart = (request: ConvertRequest) => invoke<Report>("convert_part", { request });

export const chooseFolder = (start: string) => invoke<string | null>("choose_folder", { start });

export const openFolder = (path: string) => invoke<void>("open_folder", { path });
