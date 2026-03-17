import { useEffect, useRef } from "react";
import {
    type FilteredSlashSkill,
    type SlashNamespaceItem,
} from "@/data/Slash";

interface SlashCompletionListProps {
    visible: boolean;
    placement: "top" | "bottom";
    cursorPosition: {
        bottom: number;
        left: number;
        top: number;
    };
    mode: "namespace" | "skill";
    namespaces: SlashNamespaceItem[];
    skills: FilteredSlashSkill[];
    selectedIndex: number;
    onSelectNamespace: (namespace: SlashNamespaceItem) => void;
    onSelectSkill: (skill: FilteredSlashSkill) => void;
}

const renderHighlightedText = (text: string, highlightIndices: number[]) => {
    if (highlightIndices.length === 0) {
        return text;
    }

    return text.split("").map((char, index) => (
        <span
            key={`${char}-${index}`}
            className={highlightIndices.includes(index) ? "font-bold text-primary" : ""}
        >
            {char}
        </span>
    ));
};

export default function SlashCompletionList({
    visible,
    placement,
    cursorPosition,
    mode,
    namespaces,
    skills,
    selectedIndex,
    onSelectNamespace,
    onSelectSkill,
}: SlashCompletionListProps) {
    const listRef = useRef<HTMLDivElement>(null);
    const itemsCount = mode === "namespace" ? namespaces.length : skills.length;

    useEffect(() => {
        const listElement = listRef.current;
        if (!listElement || selectedIndex < 0 || selectedIndex >= itemsCount) {
            return;
        }

        const selectedElement = listElement.querySelector(
            `.slash-completion-item:nth-child(${selectedIndex + 1})`,
        ) as HTMLElement | null;

        if (!selectedElement) {
            return;
        }

        const parentRect = listElement.getBoundingClientRect();
        const selectedRect = selectedElement.getBoundingClientRect();

        if (selectedRect.top < parentRect.top) {
            listElement.scrollTop -= parentRect.top - selectedRect.top;
        } else if (selectedRect.bottom > parentRect.bottom) {
            listElement.scrollTop += selectedRect.bottom - parentRect.bottom;
        }
    }, [itemsCount, selectedIndex]);

    if (!visible || itemsCount === 0) {
        return null;
    }

    return (
        <div
            ref={listRef}
            className="slash-completion-list"
            style={{
                ...(placement === "top"
                    ? { top: cursorPosition.top }
                    : { bottom: cursorPosition.bottom }),
                left: cursorPosition.left,
            }}
        >
            {mode === "namespace"
                ? namespaces.map((namespace, index) => (
                      <button
                          key={namespace.name}
                          type="button"
                          className={`slash-completion-item ${
                              index === selectedIndex ? "selected" : ""
                          } ${namespace.isEnabled ? "" : "disabled"}`}
                          disabled={!namespace.isEnabled}
                          onClick={() => onSelectNamespace(namespace)}
                      >
                          <div className="slash-completion-title-row">
                              <span className="slash-completion-name">
                                  {namespace.name}
                              </span>
                              {!namespace.isEnabled && (
                                  <span className="slash-completion-pill">即将支持</span>
                              )}
                          </div>
                          <div className="slash-completion-description">
                              {namespace.description}
                          </div>
                      </button>
                  ))
                : skills.map((skill, index) => (
                      <button
                          key={skill.identifier}
                          type="button"
                          className={`slash-completion-item ${
                              index === selectedIndex ? "selected" : ""
                          }`}
                          onClick={() => onSelectSkill(skill)}
                      >
                          <div className="slash-completion-title-row">
                              <span className="slash-completion-name">
                                  {renderHighlightedText(
                                      skill.invokeName,
                                      skill.highlightIndices,
                                  )}
                              </span>
                              <span className="slash-completion-pill">
                                  {skill.sourceDisplayName}
                              </span>
                          </div>
                          {skill.description && (
                              <div className="slash-completion-description">
                                  {skill.description}
                              </div>
                          )}
                          <div className="slash-completion-meta">
                              <span>{skill.identifier}</span>
                              {skill.tags.length > 0 && (
                                  <span>{skill.tags.join(", ")}</span>
                              )}
                          </div>
                      </button>
                  ))}
        </div>
    );
}
