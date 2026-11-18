import { expect, test } from "bun:test";
import { provider } from "./provider.ts";

test("the OpenAI key selects OpenAI", () => {
  expect(provider({ DEVPAGE_OPENAI_API_KEY: "k" })).toEqual({
    live: "wss://api.openai.com/v1/live/sessions",
    responses: "https://api.openai.com/v1/responses",
    headers: { authorization: "Bearer k" },
  });
});

test("the Azure endpoint selects Azure even beside an OpenAI key", () => {
  expect(provider({ DEVPAGE_OPENAI_API_KEY: "k", DEVPAGE_AZURE_OPENAI_ENDPOINT: "https://talks.openai.azure.com/", DEVPAGE_AZURE_OPENAI_API_KEY: "az" })).toEqual({
    live: "wss://talks.openai.azure.com/openai/v1/live/sessions",
    responses: "https://talks.openai.azure.com/openai/v1/responses",
    headers: { "api-key": "az" },
  });
});

test("a bare host name is an Azure endpoint", () => {
  expect(provider({ DEVPAGE_AZURE_OPENAI_ENDPOINT: "talks.openai.azure.com", DEVPAGE_AZURE_OPENAI_API_KEY: "az" })?.responses).toBe("https://talks.openai.azure.com/openai/v1/responses");
});

test("an Azure endpoint without its key is no provider", () => {
  expect(provider({ DEVPAGE_OPENAI_API_KEY: "k", DEVPAGE_AZURE_OPENAI_ENDPOINT: "talks.openai.azure.com" })).toBeNull();
});

test("blank variables are no provider", () => {
  expect(provider({ DEVPAGE_OPENAI_API_KEY: " ", DEVPAGE_AZURE_OPENAI_ENDPOINT: "" })).toBeNull();
});
