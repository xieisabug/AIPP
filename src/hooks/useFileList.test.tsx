import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AttachmentType, FileInfo } from "../data/Conversation";
import { useFileList } from "./useFileList";

function Harness({
  files,
  onDelete = () => undefined,
}: {
  files: FileInfo[];
  onDelete?: (id: number) => void;
}) {
  const { renderFiles } = useFileList(files, onDelete, () => undefined);
  return <div data-testid="list">{renderFiles()}</div>;
}

describe("useFileList attachment tray chips", () => {
  it("shows text filename, type label, and a remove control", () => {
    const onDelete = vi.fn();
    render(
      <Harness
        onDelete={onDelete}
        files={[
          {
            id: 2,
            name: "notes.txt",
            path: "/tmp/notes.txt",
            type: AttachmentType.Text,
          },
          {
            id: 1,
            name: "shot.png",
            path: "/tmp/shot.png",
            type: AttachmentType.Image,
            thumbnail: "data:image/png;base64,aaa",
          },
        ]}
      />,
    );

    expect(screen.getByText("notes.txt")).toBeTruthy();
    expect(screen.getByText("TXT")).toBeTruthy();
    expect(screen.getByAltText("shot.png")).toBeTruthy();

    const removes = screen.getAllByTitle(/移除/);
    expect(removes.length).toBe(2);
    fireEvent.click(removes[0]);
    expect(onDelete).toHaveBeenCalled();
  });
});
