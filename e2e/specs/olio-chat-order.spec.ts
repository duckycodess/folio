import { addFolder, expect, test } from "../support/app";

/**
 * The floating chat reads like a messenger: requests stack oldest to newest
 * down the panel, and sending one keeps the newest in view above the composer.
 */
test("the floating chat lists turns oldest first and follows the newest", async ({
  folio,
}) => {
  await addFolder(folio);
  await folio.getByRole("button", { name: /Talk to me/ }).click();
  const input = folio.getByRole("textbox", { name: "Message Olio" });
  const requests = ["first question", "second question", "third question"];
  for (const request of requests) {
    await input.fill(request);
    await input.press("Enter");
    await expect(
      folio.locator(".olio-chat-request q", { hasText: request }),
    ).toBeVisible();
  }

  await expect(folio.locator(".olio-chat-request q")).toHaveText(requests);

  const messages = folio.locator(".olio-chat-messages");
  await expect
    .poll(() =>
      messages.evaluate(
        (box) => box.scrollHeight - box.scrollTop - box.clientHeight,
      ),
    )
    .toBeLessThan(2);
});
