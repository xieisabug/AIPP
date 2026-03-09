interface BenchmarkQaItem {
  id: string;
  question: string;
  reference: string;
  updatedAt: string;
}

interface BenchmarkSet {
  id: string;
  name: string;
  description: string;
  judgePrompt: string;
  runnerType: "assistant" | "model";
  runnerAssistantId: string;
  runnerModelId: string;
  judgeType: "assistant" | "model";
  judgeAssistantId: string;
  judgeModelId: string;
  items: BenchmarkQaItem[];
  createdAt: string;
  updatedAt: string;
}

interface BenchmarkRunItem {
  itemId: string;
  question: string;
  reference: string;
  answer: string;
  judgeQuestion?: string;
  score: number;
  reason: string;
  judgeRaw: string;
}

interface BenchmarkRunRecord {
  id: string;
  setId: string;
  setName: string;
  createdAt: string;
  status: "success" | "failed";
  totalScore: number;
  avgScore: number;
  runnerType?: "assistant" | "model";
  runnerTargetId?: string;
  runnerLabel: string;
  judgeType?: "assistant" | "model";
  judgeTargetId?: string;
  judgeLabel: string;
  judgeSystemPrompt?: string;
  items: BenchmarkRunItem[];
  error?: string;
}

interface BenchmarkState {
  sets: BenchmarkSet[];
  runs: BenchmarkRunRecord[];
}

interface BenchmarkOption {
  value: string;
  label: string;
}
