import { createContext, useContext } from "react";

/**
 * Whether the surrounding message belongs to the latest turn. `MessageItem` provides it so cards
 * rendered inside a message (inline interaction items, tool result cards) can fold themselves once
 * the conversation moves on, the same way preview_code does. Outside a message it stays `true`.
 */
export const MessageTurnContext = createContext(true);

export const useIsLatestMessageTurn = () => useContext(MessageTurnContext);
