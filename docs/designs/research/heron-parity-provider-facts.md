# Provider facts for the Heron parity design

Used by [../heron-parity.md](../heron-parity.md) §4, §5 and §9. Researched 2026-11-23 from vendor pages, read as text. Each fact has the URL it came from. UNCONFIRMED marks anything not read on a page; the phase delta that uses it must check it again before build. Prices change, so §9 of the design doc treats these as estimates.

Research date: 2026-11-23. Pages were fetched with curl (WebFetch returned unusable output) and read as extracted text. Apple facts come from developer.apple.com documentation JSON. "Page date" is what the page itself shows; "none" means no date was visible.

## 1. Outbound phone calls by an AI agent

### Twilio
- US local number: $1.15/month. Page says "Pricing current as of October 2026". https://www.twilio.com/en-us/voice/pricing/us
- Outbound to US/Canada numbers from a local number: $0.0140/min (toll-free numbers also $0.0140/min outbound). Receiving on a local number: $0.0085/min. Same page and date. https://www.twilio.com/en-us/voice/pricing/us
- Call recording: $0.0025/min. Same page. https://www.twilio.com/en-us/voice/pricing/us
- Media Streams streams raw call audio over WebSockets. A bidirectional stream also lets the app send audio back into the call. Page date: none. https://www.twilio.com/docs/voice/media-streams
- Media Streams needs a reachable endpoint. The prerequisites say "Have a WebSocket server that accepts secure WebSocket connections (TCP port 443) from Twilio." So it must be wss on 443, which in practice means a public endpoint. A local Mac app needs a tunnel or relay. https://www.twilio.com/docs/voice/media-streams

### Hosted voice-agent platforms
- ElevenLabs (ElevenAgents): extra call minutes cost $0.08/min, and burst pricing is $0.16/min when over the concurrency limit. Text messages cost $0.003 each. LLM usage is billed separately. ElevenLabs charges nothing extra for telephony, but an external provider such as Twilio bills you directly. Free plan includes 15 minutes and 4 concurrent calls. Page date: none (the page shows a "First month for $1 ... Ends September 30" banner). https://elevenlabs.io/pricing/agents
- ElevenLabs has native Twilio integration for inbound and outbound calls. Purchased Twilio numbers are fully supported, and verified caller IDs are outbound only. https://elevenlabs.io/docs/agents-platform/phone-numbers/twilio-integration/native-integration
- ElevenLabs "transfer to number" system tool transfers the ongoing call to a phone number or SIP URI. https://elevenlabs.io/docs/agents-platform/customization/tools/system-tools/transfer-to-human
- ElevenLabs post-call webhook (`post_call_transcription`) exists, with an audio variant and a call-initiation-failure event. https://elevenlabs.io/docs/eleven-agents/workflows/post-call-webhooks
- ElevenLabs live (mid-call) transcript webhook: UNCONFIRMED. The only webhook page I read describes post-call events, so mid-call transcripts appear to need the agent WebSocket, not a webhook.
- Vapi: platform hosting fee is not stated as a rate. The page's example estimate shows $50 for 1,000 minutes, which implies $0.05/min (my inference). Provider costs are passed through with no markup. Example costs on the page: Deepgram $0.0095 to $0.0099/min, OpenAI $0.0077 to $0.0452/min, ElevenLabs voice $0.0146 to $0.0238/min. https://vapi.ai/pricing
- Vapi telephony transport fees (charged by provider): Twilio outbound $0.014/min, Twilio inbound $0.008/min, Telnyx $0.0055/min, Vonage $0.00814/min. Vapi Telephony/SIP, WebSockets and Daily WebRTC are listed as Free. The free tier includes 1 Vapi number and $5 credit. https://vapi.ai/pricing
- Vapi outbound: an outbound call needs an imported number from a supported provider (Twilio is supported). "You cannot make outbound or international calls with a free Vapi number." https://docs.vapi.ai/calls/outbound-calling
- Vapi transfer: the Transfer Call tool routes an active call to a phone number or SIP URI, with warm-transfer modes. https://docs.vapi.ai/call-forwarding
- Vapi live events: server messages sent to a Server URL include `transcript` and `conversation-update`. https://docs.vapi.ai/server-url/events
- Retell: voice agents cost $0.07 to $0.31/min on pay-as-you-go. Example breakdown: voice infra $0.055/min, TTS $0.015/min, US Twilio telephony $0.015/min. $10 free credit and 20 free concurrent calls. https://www.retellai.com/pricing
- Retell phone numbers: $2.00/month; no charge for custom telephony/SIP trunking. https://www.retellai.com/pricing
- Retell has a Create Phone Call API (outbound). https://docs.retellai.com/api-references/create-phone-call
- Retell webhooks: `call_started`, `call_ended`, `call_analyzed`, `transcript_updated` (fires on turn-taking transcript updates), and `transfer_started`. https://docs.retellai.com/features/webhook-overview
- Retell transfer to a human number: the pricing page lists "Call Transfer: Switch between human and AI" as a feature, and the webhook page shows transfer events. I did not read a transfer how-to page (the docs URL I tried returned "Page Not Found"). https://www.retellai.com/pricing

### US law
- FCC Declaratory Ruling FCC 24-17: adopted February 2, 2024, released February 8, 2024. It confirms that the TCPA's "artificial or prerecorded voice" restrictions "encompass current AI technologies that generate human voices." Read from the FCC's own PDF. https://docs.fcc.gov/public/attachments/FCC-24-17A1.pdf
- All-party consent to record: Washington RCW 9.73.030 requires "the consent of all the participants" for recording a private telephone communication. https://app.leg.wa.gov/rcw/default.aspx?cite=9.73.030
- All-party consent: Florida 934.03 makes interception lawful only when "all of the parties to the communication have given prior consent" (among other exceptions). https://www.flsenate.gov/Laws/Statutes/2024/934.03
- California all-party consent (Penal Code 632): UNCONFIRMED from a page I read. leginfo, Justia and FindLaw returned errors or Cloudflare challenges. Treat as widely reported but unverified here.
- Complete list of all-party states: UNCONFIRMED (Justia 50-state survey and the Digital Media Law Project were blocked).

## 2. Messaging

### WhatsApp Business Platform (Cloud API)
- Pricing model: "Effective July 1, 2025 – Meta charges on a per-message basis" (replacing conversation-based pricing). Meta may change rates only on Jan 1, Apr 1, Jul 1, Oct 1. Page shows rates effective October 1, 2026. https://developers.facebook.com/docs/whatsapp/pricing
- Service messages (replies within the 24-hour customer service window): free through Sep 30, 2026. As of Oct 1, 2026 Meta resumed charging for them, with a free tier of 1,000 service messages per business phone number per month (no rollover). Without a payment method on the account, messages past the free tier are not delivered. https://developers.facebook.com/docs/whatsapp/pricing
- Conflict: the marketing page still says "we do not charge for service messages". The developer docs page is more specific and dated, so I trust it. https://business.whatsapp.com/products/platform-pricing
- Per-message USD rate for a service message past the free tier: UNCONFIRMED (rate table not in the text I read).
- The 24-hour window opens when a user messages the business and resets with each user message. https://business.whatsapp.com/products/platform-pricing
- Phone number: "Numbers already in use with WhatsApp cannot be registered" for Cloud API unless migrated/deregistered per the page. A registered number can still be used for calls and texts but not for ordinary WhatsApp. https://developers.facebook.com/docs/whatsapp/cloud-api/phone-numbers
- Webhook endpoint: must handle HTTPS requests, "must have a valid TLS or SSL certificate... Self-signed certificates are not supported." So a public HTTPS endpoint (or relay) is required. https://developers.facebook.com/docs/graph-api/webhooks/getting-started
- Meta Business account requirement and "one person talking to their own assistant": UNCONFIRMED from a page. The Cloud API get-started page references business portfolios and messaging accounts, but I did not read an explicit personal-use statement.

### Slack
- Socket Mode: Slack uses a WebSocket URL, created at runtime, instead of a public HTTP Request URL. It helps developers behind firewalls. https://docs.slack.dev/apis/events-api/using-socket-mode/
- Socket Mode apps "currently can't be listed in the public Slack Marketplace." Same page.
- Free workspace: you can add up to 10 third-party or custom apps (so a custom app is allowed within the limit). https://slack.com/help/articles/115002422943-Understand-Slack-plan-limits

### Microsoft Teams bots
- Azure AI Bot Service pricing: Standard channels (Teams is listed as a standard channel) give unlimited messages on both Free and S1. Premium channels: 10,000 messages/month on Free. S1 premium price showed "$-" so UNCONFIRMED. Page date: none. https://azure.microsoft.com/en-us/pricing/details/bot-services/
- Registration: the bot identity can be single-tenant or user-assigned managed identity. "Multi-tenant bot creation will be deprecated after July 31, 2025." https://learn.microsoft.com/en-us/azure/bot-service/bot-service-quickstart-registration?view=azure-bot-service-4.0
- Public HTTPS messaging endpoint: partial. Teams docs say a locally hosted bot needs a tunneling service such as ngrok to test, which implies a public endpoint. I did not read an explicit "messaging endpoint must be HTTPS" sentence. https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/debug/locally-with-an-ide

### iMessage on macOS
- Sending via AppleScript/Shortcuts in Messages: UNCONFIRMED (no official Apple page found and read).
- Reading `~/Library/Messages/chat.db` needs Full Disk Access: UNCONFIRMED (the Apple support page I fetched did not mention it).

## 3. Meetings and system audio

- Recall.ai pay-as-you-go: $0.50/hr of recording. Built-in transcription $0.15/h. 7 days of free storage, then $0.05/hr retained for 30 days. Page date: none. https://www.recall.ai/pricing
- Recall Meeting Bot API supports Zoom, Google Meet, Microsoft Teams, Webex, GoTo Meeting and Slack Huddles, and lists "Send and receive meeting chat messages". Same page. https://www.recall.ai/pricing
- Recall has a "Send Chat Message" endpoint. Per-platform chat support is not detailed on the page I read. https://docs.recall.ai/reference/bot_send_chat_message_create
- ScreenCaptureKit `SCStreamConfiguration.capturesAudio`: macOS 13.0+. https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturesaudio
- Core Audio process taps: the sample-code page says "ensure that you're using macOS 14.2 or later". Caveat: the page metadata lists macOS 26.0 (for the sample project), and `CATapDescription` metadata lists macOS 12.0, so the API-level metadata conflicts. I trust the 14.2 text. https://developer.apple.com/documentation/CoreAudio/capturing-system-audio-with-core-audio-taps

## 4. Live voice on macOS and model prices

- `SpeechAnalyzer` and `SpeechTranscriber`: introduced in macOS 26.0 (also iOS 26). The docs show streaming analysis with volatile (provisional) results and a model-asset download via `AssetInventory`. The pages I read do not use the words "on-device" as a feature claim (only an error message about on-device resources), so treat on-device as likely but UNCONFIRMED. https://developer.apple.com/documentation/speech/speechanalyzer
- `SFSpeechRecognitionRequest.requiresOnDeviceRecognition`: available since macOS 10.15. `SFSpeechRecognizer.supportsOnDeviceRecognition` since iOS/Mac Catalyst 13.0. https://developer.apple.com/documentation/speech/sfspeechrecognitionrequest/requiresondevicerecognition
- `AVAudioIONode.setVoiceProcessingEnabled(_:)`: macOS 10.15+ ("Enables or disables voice processing on the I/O node"). The page does not itself say "echo cancellation"; that is the known purpose but unconfirmed here. https://developer.apple.com/documentation/avfaudio/avaudioionode/setvoiceprocessingenabled(_:)
- OpenAI `gpt-realtime-2.1` audio: $32.00 per 1M input tokens, $64.00 per 1M output, $0.40 cached input. `gpt-realtime-2.1-mini` audio: $10.00 / $20.00 / $0.30. Page date: none. https://developers.openai.com/api/docs/pricing
- OpenAI GPT-Live voice sessions: $0.05/min active, $0.02/min standby. Same page. https://developers.openai.com/api/docs/pricing
- Claude Haiku 5.5 (newest Haiku): $0.10 per MTok input and $0.50 per MTok output for prompts up to 100,000 tokens ($0.50 / $2.50 above that). Cache hit $0.01/MTok. https://platform.claude.com/docs/en/about-claude/pricing
- Claude Haiku 4.5: $1 input / $5 output per MTok. Same page. https://platform.claude.com/docs/en/about-claude/pricing

## 5. Payments for AI agents

- Privacy.com: plans are Personal $0 (12 new cards/month), Plus $5/mo (24 cards, "API Access"), Pro $10/mo, Premium $25/mo. So API access starts on the paid Plus plan; the free personal plan does not list it. The developer docs describe a public API plus an MCP server and CLI, and say the account holder is responsible for all API/CLI/MCP activity. https://privacy.com/pricing and https://developers.privacy.com/
- Stripe Issuing: docs describe it as creating cards "for your business". Commercial issuing is in the US, Canada, UK and supported EEA countries. Consumer issuing is US and is a program model for platforms. Whether an individual with no business can use it: UNCONFIRMED (no explicit statement read). https://docs.stripe.com/issuing
- Stripe agentic commerce: "Build agents that pay, or sell products, services, and APIs to agents." Integrations include Link Agent Wallet, Shared Payment Tokens, Universal Commerce Protocol, machine payments (MPP, x402). "Availability, launch stage, and supported countries vary by integration." https://docs.stripe.com/agentic-commerce
- Stripe Link Agent Wallet: agent payments available to US and Canadian customers; financial insights US only. It uses the Link API (api.link.com) with a per-customer OAuth token, not Stripe API keys. A search result said Stripe's terms still label it "Preview" (not verified by me). https://docs.stripe.com/agentic-commerce/agents/link-agent-wallet
- Visa Intelligent Commerce: developer page open to independent developers for sandbox via a Visa Developer account, but production needs onboarding to VIC, Trusted Agent Protocol, Visa Token Service, Visa Payment Passkeys, and the Visa Agent Registry. https://developer.visa.com/capabilities/visa-intelligent-commerce
- Mastercard Agent Pay: UNCONFIRMED. mastercard.com returned Access Denied, and developer.mastercard.com pages needed JavaScript. A search snippet says integrators must register with Mastercard and mostly register as Token Requestors, but I did not read that page. https://developer.mastercard.com/mastercard-checkout-solutions/documentation/use-cases/agent-pay/
- Shop Pay for agents: Shopify's "Shop MCP" lets agents search catalogs, build carts and check out. No API key or UCP profile is needed. Without Shop sign-in the buyer pays on the merchant checkout page; with Shop sign-in the agent completes checkout after the buyer confirms the total and payment method. https://shopify.dev/docs/agents
- Apple LocalAuthentication `LAContext`: available on macOS 10.10+. The page describes it as evaluating authentication policies; Touch ID specifics are not stated in the abstract I read. https://developer.apple.com/documentation/localauthentication/lacontext

## 6. Instagram API

- The Instagram Platform serves "Instagram professional accounts, including business and creator accounts". Instagram Login (no Facebook Page needed) is recommended for new integrations. https://developers.facebook.com/docs/instagram-platform/overview
- App Review: an app only for a business you own or manage uses Standard Access and App Review is "Not required". A Tech Provider app serving multiple businesses needs Advanced Access and App Review is "Required". Page date: none. https://developers.facebook.com/docs/instagram-platform/app-review
- Content publishing is for Instagram professional accounts (single images, videos, reels, carousels). https://developers.facebook.com/docs/instagram-platform/content-publishing
- Messaging: an app can message a user only after that user has messaged the professional account first, within a standard window that is 24 hours for most user actions. https://developers.facebook.com/docs/instagram-platform/instagram-api-with-instagram-login/messaging-api
- Whether publishing/messaging for a personal single-owner app needs a separate review: the review table suggests no (Standard Access), but I did not read a per-permission statement. Treat as likely, UNCONFIRMED.

## 7. Relays

- Workers Free: 100,000 requests/day and 100,000 subrequests (page lists Workers Free limits). https://developers.cloudflare.com/workers/platform/limits/ and https://developers.cloudflare.com/workers/platform/pricing/
- Durable Objects are available on both Workers Free and Paid, but on Free only with the SQLite storage backend. Daily free limits reset at 00:00 UTC, and operations fail once a limit is exceeded. https://developers.cloudflare.com/durable-objects/platform/pricing/
- Free-plan Durable Object WebSocket specifics (duration/GB-s limits): UNCONFIRMED (not extracted).
- Cloudflare Tunnel (cloudflared): the account-limits page lists Cloudflare Tunnel under the Zero Trust Free plan, which implies it is available free. I did not find an explicit "Tunnel is free" sentence, so mark as likely, UNCONFIRMED. https://developers.cloudflare.com/cloudflare-one/account-limits/

## Tooling note
WebFetch returned empty/unusable results for every call, so all pages were retrieved with curl and text extraction. Pages behind Cloudflare/Akamai challenges (Justia, fcc.gov HTML, mastercard.com, leginfo) could not be read.
