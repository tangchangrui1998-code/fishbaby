import { searchText } from "./navigation";
let text = "";
let bookId = "";
self.onmessage = (event: MessageEvent) => {
  const request = event.data;
  if (request.type === "book") {
    text = request.text;
    bookId = request.bookId;
    return;
  }
  self.postMessage({
    ...searchText(text, request.query, request.from),
    id: request.id,
    bookId,
  });
};
