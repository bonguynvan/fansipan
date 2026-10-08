import { describe, expect, it } from "vitest";
import { activeTicket, registerTicket, useTicket } from "./tickets.svelte.js";

describe("tickets", () => {
  it("reaches the ticket used last, else the first opened, else none", () => {
    expect(activeTicket()).toBeNull();
    const first = { name: "first" };
    const second = { name: "second" };
    const closeFirst = registerTicket(first);
    const closeSecond = registerTicket(second);

    expect(activeTicket()).toBe(first);
    useTicket(second);
    expect(activeTicket()).toBe(second);

    closeSecond();
    expect(activeTicket()).toBe(first);
    closeFirst();
    expect(activeTicket()).toBeNull();
  });
});
