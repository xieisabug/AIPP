/// <reference types="react" />

type AippButtonVariant =
  | "default"
  | "destructive"
  | "outline"
  | "secondary"
  | "ghost"
  | "link";
type AippButtonSize = "default" | "sm" | "lg" | "icon";
type AippBadgeVariant = "default" | "secondary" | "destructive" | "outline";

interface AippSystemApiAssistantItem {
  id: number;
  name: string;
  assistant_type: number;
}

interface AippSystemApiModelItem {
  id: number;
  name: string;
  code: string;
  llm_provider_id: number;
}

interface AippSystemApiRunTextUsage {
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
  total_tokens?: number | null;
}

interface AippSystemApiRunTextResult {
  content: string;
  model: string;
  usage?: AippSystemApiRunTextUsage | null;
}

interface AippSystemApiRunAssistantTextOptions {
  assistantId: number | string;
  prompt: string;
  systemPrompt?: string;
  context?: string;
}

interface AippSystemApiRunModelTextOptions {
  modelId: string;
  prompt: string;
  systemPrompt?: string;
  context?: string;
}

interface AippSystemApiSqlQueryRequest {
  sql: string;
  params?: unknown[];
  maxRows?: number;
}

interface AippSystemApiDataQueryRequest extends AippSystemApiSqlQueryRequest {
  database: string;
}

interface AippSystemApiSqlQueryResult {
  columns: string[];
  rows: unknown[][];
  rowCount: number;
  truncated: boolean;
}

interface AippSystemApiSqlExecuteRequest {
  sql: string;
  params?: unknown[];
}

interface AippSystemApiSqlExecuteResult {
  rowsAffected: number;
  lastInsertRowid: number;
}

interface AippSystemApiDatabaseColumnSchema {
  name: string;
  dataType: string;
  notNull: boolean;
  defaultValue?: string | null;
  primaryKey: boolean;
}

interface AippSystemApiDatabaseTableSchema {
  name: string;
  objectType: string;
  sql?: string | null;
  columns: AippSystemApiDatabaseColumnSchema[];
}

interface AippSystemApiDatabaseSchema {
  database: string;
  tables: AippSystemApiDatabaseTableSchema[];
}

interface AippSystemApiData {
  query(request: AippSystemApiDataQueryRequest): Promise<AippSystemApiSqlQueryResult>;
  schema(database: string): Promise<AippSystemApiDatabaseSchema>;
}

interface AippSystemApiStorage {
  query(request: AippSystemApiSqlQueryRequest): Promise<AippSystemApiSqlQueryResult>;
  execute(request: AippSystemApiSqlExecuteRequest): Promise<AippSystemApiSqlExecuteResult>;
  schema(): Promise<AippSystemApiDatabaseSchema>;
}

interface AippSystemApiAssistantConfig {
  get(assistantId: number | string, key: string): Promise<string | null>;
  getAll(assistantId: number | string): Promise<Record<string, string | null>>;
  set(assistantId: number | string, key: string, value: string | null): Promise<void>;
}

interface AippSystemApiMessage {
  id: number;
  conversation_id: number;
  message_type: string;
  content: string;
  llm_model_id?: number | null;
  llm_model_name?: string | null;
  created_time: string;
  token_count: number;
  input_token_count?: number | null;
  output_token_count?: number | null;
  tool_calls_json?: string | null;
  metadata_json?: string | null;
}

interface AippSystemApiActions {
  createConversation(request: {
    assistantId: number;
    conversationName?: string;
  }): Promise<number>;
  appendMessage(request: {
    conversationId: number;
    messageType: string;
    content: string;
    metadata?: unknown;
  }): Promise<AippSystemApiMessage>;
  updateMessageMetadata(request: {
    messageId: number;
    metadata?: unknown;
  }): Promise<AippSystemApiMessage>;
}

type AippSystemApiHookAction =
  | "continue"
  | "replace"
  | "patch"
  | "block"
  | "approvalRequired";

interface AippSystemApiHookResult {
  action?: AippSystemApiHookAction;
  context?: unknown;
  patch?: unknown;
  message?: string | null;
  metadata?: unknown;
}

type AippSystemApiHookHandler = (
  context: unknown
) => AippSystemApiHookResult | void | Promise<AippSystemApiHookResult | void>;

interface AippSystemApiHooks {
  register(hookName: string, handler: AippSystemApiHookHandler): void;
  unregister(hookName: string): void;
}

type AippSystemApiThemeMode = "light" | "dark" | "both";

interface AippSystemApiThemeDefinition {
  id: string;
  label: string;
  mode?: AippSystemApiThemeMode;
  variables: Record<string, string>;
  description?: string;
  /**
   * Optional global CSS snippet.
   * Use `:scope` as theme root placeholder; for precise targeting prefer
   * `[data-aipp-slot="..."]` selectors.
   */
  extraCss?: string;
  /**
   * Optional per-window CSS snippets.
   * Key is window label (`chat_ui`, `ask`, etc). In snippet, `:scope` maps to
   * `.theme-<id>.aipp-window-<label>`.
   */
  windowCss?: Record<string, string>;
}

interface AippSystemApiDisplayConfig {
  theme: string;
  color_mode: string;
  user_message_markdown_render: string;
  code_theme_light: string;
  code_theme_dark: string;
}

interface AippSystemApiMarkdownTagRendererProps {
  node?: unknown;
  children?: React.ReactNode;
  attributes: Record<string, string>;
  props: Record<string, unknown>;
}

type AippSystemApiMarkdownTagRenderer = (
  props: AippSystemApiMarkdownTagRendererProps
) => React.ReactNode;

interface AippSystemApiMarkdownTagRegistration {
  tagName: string;
  attributes?: string[];
  render: AippSystemApiMarkdownTagRenderer;
}

/**
 * Host-provided UI kit components exposed to plugins.
 * This allows IDE autocomplete for available components + core props.
 */
interface AippSystemApiUiKit {
  Alert?: React.ComponentType<React.ComponentProps<"div"> & { variant?: "default" | "destructive" }>;
  AlertDescription?: React.ComponentType<React.ComponentProps<"div">>;
  AlertTitle?: React.ComponentType<React.ComponentProps<"div">>;
  Badge?: React.ComponentType<React.ComponentProps<"span"> & { variant?: AippBadgeVariant }>;
  Button?: React.ComponentType<
    React.ComponentProps<"button"> & {
      variant?: AippButtonVariant;
      size?: AippButtonSize;
      asChild?: boolean;
    }
  >;
  Card?: React.ComponentType<React.ComponentProps<"div">>;
  CardContent?: React.ComponentType<React.ComponentProps<"div">>;
  CardDescription?: React.ComponentType<React.ComponentProps<"div">>;
  CardFooter?: React.ComponentType<React.ComponentProps<"div">>;
  CardHeader?: React.ComponentType<React.ComponentProps<"div">>;
  CardTitle?: React.ComponentType<React.ComponentProps<"div">>;
  Dialog?: React.ComponentType<{
    open?: boolean;
    defaultOpen?: boolean;
    onOpenChange?: (open: boolean) => void;
    children?: React.ReactNode;
  }>;
  DialogContent?: React.ComponentType<React.ComponentProps<"div"> & { showCloseButton?: boolean }>;
  DialogDescription?: React.ComponentType<React.ComponentProps<"div">>;
  DialogHeader?: React.ComponentType<React.ComponentProps<"div">>;
  DialogTitle?: React.ComponentType<React.ComponentProps<"div">>;
  Input?: React.ComponentType<React.ComponentProps<"input">>;
  Textarea?: React.ComponentType<React.ComponentProps<"textarea">>;
  Select?: React.ComponentType<{
    value?: string;
    onValueChange?: (value: string) => void;
    children?: React.ReactNode;
  }>;
  SelectTrigger?: React.ComponentType<React.ComponentProps<"button">>;
  SelectValue?: React.ComponentType<{ placeholder?: string }>;
  SelectContent?: React.ComponentType<{ children?: React.ReactNode }>;
  SelectItem?: React.ComponentType<{ value: string; children?: React.ReactNode }>;
}

interface SystemApi {
  pluginId: number;
  pluginCode: string;
  listAssistants(): Promise<AippSystemApiAssistantItem[]>;
  listModels(): Promise<AippSystemApiModelItem[]>;
  getData(key: string, sessionId?: string): Promise<string | null>;
  getAllData(sessionId?: string): Promise<Record<string, string | null>>;
  setData(key: string, value: string | null, sessionId?: string): Promise<void>;
  runAssistantText(
    options: AippSystemApiRunAssistantTextOptions
  ): Promise<AippSystemApiRunTextResult>;
  runModelText(
    options: AippSystemApiRunModelTextOptions
  ): Promise<AippSystemApiRunTextResult>;
  registerTheme(theme: AippSystemApiThemeDefinition): void;
  unregisterTheme(themeId: string): void;
  listThemes(): Promise<AippSystemApiThemeDefinition[]>;
  registerMarkdownTag(registration: AippSystemApiMarkdownTagRegistration): void;
  unregisterMarkdownTag(tagName: string): void;
  listMarkdownTags(): Promise<AippSystemApiMarkdownTagRegistration[]>;
  hooks: AippSystemApiHooks;
  data: AippSystemApiData;
  storage: AippSystemApiStorage;
  assistantConfig: AippSystemApiAssistantConfig;
  actions: AippSystemApiActions;
  getDisplayConfig(): Promise<AippSystemApiDisplayConfig>;
  applyTheme(themeId: string): Promise<void>;
  ui?: AippSystemApiUiKit;
  invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T>;
}

interface AippPlugin {
  config?(): { name?: string; type?: string[] };
  onPluginLoad?(systemApi: SystemApi): void | Promise<void>;
  renderComponent?(): React.ReactNode;
  renderView?(viewId: string, context?: Record<string, unknown>): React.ReactNode;
}
