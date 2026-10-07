import { describe, expect, it } from "vitest";
import { TAPE_PRINTS, printRows, withPrints, withTapeHistory } from "./tape.js";

const print = (symbol, seq, price = "62500") => ({
  symbol,
  seq,
  price,
  quantity: "100",
  side: "buy",
  time_ms: seq * 1000,
});
const seqs = (tape) => tape.map((p) => p.seq);

describe("withPrints", () => {
  it("appends each symbol's prints in tape order", () => {
    const tapes = withPrints({}, [print("FPT", 1), print("VNM", 4), print("FPT", 2)]);

    expect(seqs(tapes.FPT)).toEqual([1, 2]);
    expect(seqs(tapes.VNM)).toEqual([4]);
  });

  it("keeps a print the history read already brought once", () => {
    const tapes = withPrints({ FPT: [print("FPT", 1), print("FPT", 2)] }, [print("FPT", 2), print("FPT", 3)]);

    expect(seqs(tapes.FPT)).toEqual([1, 2, 3]);
  });

  it("keeps tape order whatever order the prints arrive in", () => {
    const tapes = withPrints({ FPT: [print("FPT", 5)] }, [print("FPT", 7), print("FPT", 6)]);

    expect(seqs(tapes.FPT)).toEqual([5, 6, 7]);
  });

  it("keeps only the newest prints", () => {
    const many = Array.from({ length: TAPE_PRINTS + 5 }, (_, i) => print("FPT", i + 1));

    const tape = withPrints({}, many).FPT;

    expect(tape).toHaveLength(TAPE_PRINTS);
    expect(tape[0].seq).toBe(6);
  });

  it("returns the same tapes for a batch without prints", () => {
    const tapes = { FPT: [print("FPT", 1)] };

    expect(withPrints(tapes, [])).toBe(tapes);
  });
});

describe("withTapeHistory", () => {
  it("puts the read history under live prints, overlap once", () => {
    const live = withPrints({}, [print("FPT", 9), print("FPT", 10)]);

    const tapes = withTapeHistory(live, "FPT", [print("FPT", 7), print("FPT", 8), print("FPT", 9)]);

    expect(seqs(tapes.FPT)).toEqual([7, 8, 9, 10]);
  });

  it("leaves other symbols alone", () => {
    const live = withPrints({}, [print("VNM", 3)]);

    const tapes = withTapeHistory(live, "FPT", [print("FPT", 1)]);

    expect(seqs(tapes.VNM)).toEqual([3]);
  });
});

describe("printRows", () => {
  const board = { reference: "62500", ceiling: "66800", floor: "58200" };

  it("lists the newest print first, scaled for display", () => {
    const rows = printRows([print("VNM", 1, "62500"), print("VNM", 2, "62600")], board, 1000);

    expect(rows.map((r) => [r.id, r.price])).toEqual([
      [2, 62.6],
      [1, 62.5],
    ]);
  });

  it("tones each price against the day's band", () => {
    const tape = ["66800", "58200", "62500", "63000", "62000"].map((price, i) => print("VNM", i + 1, price));

    const tones = printRows(tape, board).reverse().map((r) => r.tone);

    expect(tones).toEqual(["ceiling", "floor", "ref", "up", "down"]);
  });

  it("leaves prices untoned without a board", () => {
    expect(printRows([print("VNM", 1)], undefined)[0].tone).toBeUndefined();
  });
});
