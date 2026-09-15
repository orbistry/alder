import { answer as base } from "../client.js";
export async function answer() { return (await base()) + 1; }
