var BenchmarkPlugin = class BenchmarkPlugin {
  systemApi: SystemApi | null;

  constructor() {
    this.systemApi = null;
  }

  config() {
    return {
      name: "Benchmark Plugin",
      type: ["interfaceType", "applicationType"],
    };
  }

  onPluginLoad(systemApi: SystemApi) {
    this.systemApi = systemApi || null;
  }

  renderComponent() {
    const React = (window as any).React;
    if (!React) {
      return null;
    }
    return React.createElement(BenchmarkPanel, { systemApi: this.systemApi });
  }
};
