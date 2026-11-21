import { invoke } from "@tauri-apps/api/core";
export interface EvidenceSource { id: string; group: string; recordId?: string; artifactId?: string; rowIndex?: number; stage: string; source: string; body: string; chunk: number; methods: string[]; verified?: boolean; label?: string }
export interface ChatMessage { id: string; role: "user" | "assistant"; body: string; sources: EvidenceSource[]; warnings: string[]; createdAt?: string }
export interface Conversation { id: string; title: string; updatedAt: string }
export interface ChatReply { conversationId: string; message: ChatMessage }
export interface ChatEvent { requestId: string; database: string; event: { phase?: string; conversationId?: string; delta?: string; sources?: EvidenceSource[]; used?: string[] } }
export const listConversations = (database: string) => invoke<{ conversations: Conversation[] }>("case_chat_history", { database, conversationId: null });
export const loadConversation = (database: string, conversationId: string) => invoke<{ messages: ChatMessage[] }>("case_chat_history", { database, conversationId });
export const deleteConversation = (database: string, conversationId: string) => invoke("delete_case_chat", { database, conversationId });
export const stopChat = (requestId: string) => invoke("cancel_case_chat", { requestId });
