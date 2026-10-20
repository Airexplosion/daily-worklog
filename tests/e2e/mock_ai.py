"""模拟 OpenAI 兼容接口：记录请求，按内容返回识图结果或流式总结。"""
import json, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = sys.argv[1]
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 18080


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        with open(LOG, "a", encoding="utf-8") as f:
            slim = json.loads(json.dumps(body))
            for m in slim.get("messages", []):
                if isinstance(m.get("content"), list):
                    for part in m["content"]:
                        if part.get("type") == "image_url":
                            url = part["image_url"]["url"]
                            part["image_url"]["url"] = url[:40] + f"...({len(url)} chars)"
            f.write(json.dumps({"auth": self.headers.get("Authorization"), "path": self.path, "body": slim}, ensure_ascii=False) + "\n")
        if self.headers.get("Authorization") != "Bearer test-key":
            return self.reply(401, {"error": {"message": "Incorrect API key provided"}})
        user = json.dumps(body["messages"][-1], ensure_ascii=False)
        last = body["messages"][-1].get("content")
        images = sum(1 for part in last if part.get("type") == "image_url") if isinstance(last, list) else 0
        if images > 6:
            return self.reply(400, {"error": {"message": "Too many images in one request"}})
        if body.get("stream"):
            return self.stream(["1.（测试条目一）\n", "2.（测试条目", "二）"])
        system = json.dumps(body["messages"][0], ensure_ascii=False)
        if "归纳出" in system:
            content = '["分类甲","分类乙"]'
        else:
            content = '<think>测试用的思考内容</think>{"item":"（测试条目）","tags":["分类甲"],"confidence":0.42}'
        time.sleep(1.0)
        self.reply(200, {"choices": [{"message": {"role": "assistant", "content": content}}]})

    def reply(self, code, obj):
        data = json.dumps(obj, ensure_ascii=False).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def stream(self, parts):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for p in parts:
            chunk = {"choices": [{"delta": {"content": p}}]}
            self.wfile.write(f"data: {json.dumps(chunk, ensure_ascii=False)}\n\n".encode("utf-8"))
            self.wfile.flush()
            time.sleep(0.3)
        self.wfile.write(b"data: [DONE]\n\n")


ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
