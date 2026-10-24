var COMFYUI_IMAGE_WORKFLOW: Record<string, any> = {
  "9": { "inputs": { "filename_prefix": "z-image-turbo", "images": ["57:8", 0] }, "class_type": "SaveImage", "_meta": { "title": "保存图像" } },
  "57:30": { "inputs": { "clip_name": "qwen_3_4b.safetensors", "type": "lumina2", "device": "default" }, "class_type": "CLIPLoader", "_meta": { "title": "加载CLIP" } },
  "57:29": { "inputs": { "vae_name": "ae.safetensors" }, "class_type": "VAELoader", "_meta": { "title": "加载VAE" } },
  "57:33": { "inputs": { "conditioning": ["57:27", 0] }, "class_type": "ConditioningZeroOut", "_meta": { "title": "条件零化" } },
  "57:8": { "inputs": { "samples": ["57:3", 0], "vae": ["57:29", 0] }, "class_type": "VAEDecode", "_meta": { "title": "VAE解码" } },
  "57:28": { "inputs": { "unet_name": "z_image_turbo_bf16.safetensors", "weight_dtype": "default" }, "class_type": "UNETLoader", "_meta": { "title": "UNet加载器" } },
  "57:27": { "inputs": { "text": "穿黑丝的杨幂", "clip": ["57:30", 0] }, "class_type": "CLIPTextEncode", "_meta": { "title": "CLIP文本编码" } },
  "57:13": { "inputs": { "width": 1040, "height": 1024, "batch_size": 1 }, "class_type": "EmptySD3LatentImage", "_meta": { "title": "空Latent图像（SD3）" } },
  "57:11": { "inputs": { "shift": 3, "model": ["57:28", 0] }, "class_type": "ModelSamplingAuraFlow", "_meta": { "title": "采样算法（AuraFlow）" } },
  "57:3": { "inputs": { "seed": 1047870638845959, "steps": 8, "cfg": 1, "sampler_name": "res_multistep", "scheduler": "simple", "denoise": 1, "model": ["57:11", 0], "positive": ["57:27", 0], "negative": ["57:33", 0], "latent_image": ["57:13", 0] }, "class_type": "KSampler", "_meta": { "title": "K采样器" } }
};

type ComfyPromptNode = { id: string; title: string; inputs: string[] };

function comfyUiDefaultWorkflowJson(): string {
  return JSON.stringify(COMFYUI_IMAGE_WORKFLOW);
}

function comfyUiParseWorkflow(raw: string): { workflow: Record<string, any>; nodes: ComfyPromptNode[] } {
  var text = String(raw || "").trim();
  if (!text) {
    throw new Error("请上传 ComfyUI API JSON");
  }
  var parsed: any;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error("ComfyUI API JSON 无法解析");
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("ComfyUI API JSON 必须是节点对象");
  }
  if (Array.isArray(parsed.nodes) || Array.isArray(parsed.links)) {
    throw new Error("这是 ComfyUI 界面工作流，请改用 Export (API) 导出的 JSON");
  }
  var nodes: ComfyPromptNode[] = [];
  Object.keys(parsed).forEach(function (id) {
    var node = parsed[id];
    if (!node || typeof node !== "object" || Array.isArray(node) || !node.inputs || typeof node.inputs !== "object" || Array.isArray(node.inputs)) {
      return;
    }
    var inputs: string[] = [];
    Object.keys(node.inputs).forEach(function (name) {
      if (typeof node.inputs[name] === "string") inputs.push(name);
    });
    if (!inputs.length) return;
    var title = node._meta && node._meta.title ? String(node._meta.title) : String(node.class_type || id);
    nodes.push({ id: id, title: title, inputs: inputs });
  });
  if (!nodes.length) {
    throw new Error("这份 JSON 里没有可写入提示词的字符串参数");
  }
  return { workflow: parsed, nodes: nodes };
}

function comfyUiBuildWorkflow(prompt: string, promptNodeId = "57:27", promptInputName = "text", workflowSource?: Record<string, any>): Record<string, any> {
  var workflow = JSON.parse(JSON.stringify(workflowSource || COMFYUI_IMAGE_WORKFLOW));
  var node = workflow[promptNodeId];
  if (!node || !node.inputs) {
    throw new Error("工作流缺少节点 " + promptNodeId);
  }
  if (typeof node.inputs[promptInputName] !== "string") {
    throw new Error("工作流节点 " + promptNodeId + " 缺少字符串参数 " + promptInputName);
  }
  node.inputs[promptInputName] = prompt;
  return workflow;
}
