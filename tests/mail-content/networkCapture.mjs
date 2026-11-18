/** Abort and record any request to tracker.invalid (images, CSS, fonts, frames, etc.). */
export async function captureTrackerRequests(page) {
  const requested = [];
  await page.route('https://tracker.invalid/**', (route) => {
    requested.push({
      url: route.request().url(),
      resourceType: route.request().resourceType(),
    });
    return route.abort();
  });
  return requested;
}

export function assertNoRequestsBeforeAuthorize(requested) {
  if (requested.length !== 0) {
    throw new Error(`unexpected remote requests before authorize: ${JSON.stringify(requested)}`);
  }
}
