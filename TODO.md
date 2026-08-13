# Erno — Roadmap TODO

## Framework-level addressability

iOS App Store requirements were reviewed against what Erno (API + `erno-angular` + `erno new` scaffold)
can actually own. Each mobile/compliance item below is tagged:

- **[FW]** — Erno delivers it end to end (API endpoint + Angular service, or CLI behaviour). App gets it for free.
- **[FW-scaffold]** — Erno generates the file/config/template; the app developer fills in product-specific values
  (bundle IDs, copy, App Store Connect settings) that the framework cannot know.
- **[APP]** — Account-holder / submission responsibility. Erno can only document and provide checklists;
  it cannot satisfy these from code.

## DX gaps

- [ ] **Shared TypeScript types** — Publish generated or hand-authored API response types as `erno-angular/types` to avoid shape duplication across apps

## Mobile features

- [ ] **[FW]** **Social / OAuth login** — Apple Sign-In (App Store required when any other third-party login is offered), Google Sign-In; OAuth + Apple identity-token verification (JWKS) in API, `loginWithGoogle()` / `loginWithApple()` in `ErnoAuthService` via Capacitor plugins, updated auth templates in `erno new`
- [ ] **[FW]** **Push notifications** — APNs + FCM delivery in API (`notifications` module), device token registration, `ErnoNotificationsService` wrapping Capacitor push plugin, permission request (with pre-permission priming) in scaffold template
- [ ] **[FW]** **Local notifications** — `ErnoLocalNotificationsService` wrapping `@capacitor/local-notifications` for scheduled reminders and alerts that don't require a server or APNs/FCM
- [ ] **[FW]** **Haptic feedback** — `ErnoHapticsService` wrapping `@capacitor/haptics`; table-stakes mobile UX for confirming actions, errors, and destructive operations
- [ ] **[FW]** **Network awareness in sync** — Integrate `@capacitor/network` into `ErnoSyncService` and `ErnoRealtimeService` so sync pauses when offline, reconnects automatically when connectivity returns, and `status$` reflects real network state
- [ ] **[FW]** **App state handling (background/foreground)** — Listen to `@capacitor/app` `appStateChange` events in `ErnoRealtimeService` and `ErnoSyncService` to disconnect WebSocket on background and reconnect + pull delta on foreground resume
- [ ] **[FW]** **Secure token storage** — Replace `localStorage` for refresh tokens with native Keychain (iOS) / Keystore (Android) on Capacitor; `ErnoAuthService` should detect environment and use `capacitor-secure-storage-plugin` or equivalent
- [ ] **[FW-scaffold]** **Splash screen & safe area** — Configure `@capacitor/splash-screen` and safe area CSS variables in `erno new` template so apps look correct on notched / Dynamic Island iPhones and Android cutout displays (app supplies its own splash/icon assets)

## Mobile DX gaps

- [ ] **[FW-scaffold]** **Deep links in scaffold** — Add Capacitor universal links / deep-link config to `erno new` template so password reset and email verify work on physical devices out of the box (app supplies its associated-domain / app-site-association)
- [x] **[FW]** **Physical device dev** — Add `ionic cap run` option to `erno dev` command for live-reload on physical iOS/Android devices

## Mobile / platform compliance

- [ ] **[FW]** **Account & data deletion** — `DELETE /api/account` endpoint that wipes all user data + confirmation UI in scaffold; hard App Store requirement since Jan 2023, will cause rejection without it
- [ ] **[FW]** **Account data export (GDPR / CCPA)** — `GET /api/account/export` companion to deletion that returns the user's profile, synced records, and file references as a downloadable archive; supports the "right to access" and App Store data-handling expectations. Pairs with account deletion above
- [ ] **[FW]** **Native IAP (in-app purchases)** — StoreKit 2 (iOS) + Play Billing (Android) for digital goods and subscriptions; Stripe-only billing violates App Store guidelines for digital content, `ErnoIapService` wrapping RevenueCat or direct Capacitor plugin, unified entitlement check across IAP + Stripe
- [ ] **[FW-scaffold]** **iOS Privacy Manifest** — Add `PrivacyInfo.xcprivacy` to the Capacitor iOS project in `erno new` template declaring the privacy-sensitive APIs Erno itself uses (e.g. `UserDefaults`); required for iOS 17+ or App Store submission will be rejected. App merges its own API-usage reasons
- [ ] **[FW-scaffold]** **Info.plist permission purpose strings** — Pre-fill `NSFaceIDUsageDescription` (biometric Keychain unlock), plus `NSCameraUsageDescription` / `NSPhotoLibraryUsageDescription` (storage uploads) and any push-related strings in the scaffold; missing usage strings are an automatic rejection for the corresponding API
- [ ] **[FW-scaffold]** **Encryption export compliance** — Set `ITSAppUsesNonExemptEncryption = false` in the scaffold Info.plist (Erno uses only standard HTTPS/exempt crypto) so App Store Connect does not block every build asking for an export-compliance answer
- [ ] **[FW-scaffold]** **App Transport Security / HTTPS** — Ensure the scaffold ships ATS-clean (HTTPS-only API base URL in prod config, no global ATS exceptions) and document the localhost-dev exception so apps don't ship debug ATS holes
- [ ] **[FW-scaffold]** **Privacy policy & terms pages** — Scaffold placeholder privacy-policy and terms routes/pages and surface their URLs in auth/settings; App Store requires a reachable privacy policy URL for any app with accounts (app fills in the legal copy)
- [ ] **[APP]** **Privacy data declarations** — Document what Erno collects by default (email, device tokens, stored files, sync data) and provide a pre-filled template/checklist for iOS Nutrition Labels and Android Data Safety; the actual declaration is submitted by the account holder per app
- [ ] **[APP]** **App Tracking Transparency (ATT)** — Document that Erno does no cross-app tracking by default (so no ATT prompt / `NSUserTrackingUsageDescription` is required); flag for revisiting if any analytics/ad SDK is added later
