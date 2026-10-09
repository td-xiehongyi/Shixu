import type { EvidenceBlock } from "../contracts/domain";
export function SourceEvidence({ blocks }: { blocks: EvidenceBlock[] }) {
  return (
    <div className="source-evidence">
      <p className="muted">
        来源摘录（每段最多 4096 字节；完整证据保留在本机）
      </p>
      {blocks.map((b, i) => (
        <section key={`${b.part_id}-${i}`}>
          <small>
            {{ native_text: "正文", ocr: "OCR 识别", cell: "单元格" }[b.method]}{" "}
            · {b.engine_version}{" "}
            {b.page_or_sheet &&
              (b.page_or_sheet.kind === "sheet"
                ? `工作表 ${b.page_or_sheet.name}`
                : `${b.page_or_sheet.kind === "page" ? "页" : "段落"} ${b.page_or_sheet.number}`)}{" "}
            {b.cell_range_or_bbox?.kind === "cell_range" &&
              b.cell_range_or_bbox.range}{" "}
            {b.cell_range_or_bbox?.kind === "text_span" &&
              `字符 ${b.cell_range_or_bbox.start}–${b.cell_range_or_bbox.end}`}
            {b.cell_range_or_bbox?.kind === "bounding_box" &&
              `区域 (${b.cell_range_or_bbox.x},${b.cell_range_or_bbox.y},${b.cell_range_or_bbox.width},${b.cell_range_or_bbox.height})`}
            {b.quality_flags
              .map(
                (f) =>
                  ({
                    uncertain_date: "日期不确定",
                    ambiguous_layout: "版面含糊",
                    formula_derived: "公式结果",
                    partial_source: "来源不完整",
                  })[f],
              )
              .join(" · ")}
          </small>
          <pre>{b.text}</pre>
        </section>
      ))}
    </div>
  );
}
