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

function comfyUiBuildWorkflow(prompt: string, promptNodeId = "57:27", promptInputName = "text"): Record<string, any> {
  var workflow = JSON.parse(JSON.stringify(COMFYUI_IMAGE_WORKFLOW));
  var node = workflow[promptNodeId];
  if (!node || !node.inputs) {
    throw new Error("固定 workflow 缺少节点 " + promptNodeId);
  }
  if (typeof node.inputs[promptInputName] !== "string") {
    throw new Error("固定 workflow 节点 " + promptNodeId + " 缺少字符串参数 " + promptInputName);
  }
  node.inputs[promptInputName] = prompt;
  return workflow;
}
