# Prior art: MTG draft bots and deck evaluation (condensed, 2026-10-21)

Condensed from a research agent's report (sources fetched; unverified items marked).

## Most useful for mtg-zero

* **17lands Arena "Cube - Powered" dataset (CC BY 4.0), with game outcomes.**
  `https://17lands-public.s3.amazonaws.com/analysis_data/{draft_data,game_data,replay_data}/{type}_public.Cube_-_Powered.{PremierDraft,TradDraft}.csv.gz`
  * PremierDraft draft_data: 93 MB gz, 2,118,673 picks / 48,152 drafts (2025-10-30 to 11-19);
    TradDraft: 8.7 MB, 197,999 picks / 4,500 drafts. 545 card columns, 3 packs of 15.
  * game_data (27 MB / 2.9 MB) has `won`, `deck_*`, `main_colors`, ranks, user win-rate bucket,
    joinable by `draft_id`. replay_data (143 MB / 15 MB) per turn.
  * draft_data columns: `pack_number, pick_number, pick, pick_maindeck_rate,
    event_match_wins/losses, rank, user_game_win_rate_bucket`, one-hot `pack_card_<name>`,
    `pool_<name>` counts. The Arena card list differs from PowerLSV.
* **Human-pick agreement ceiling on cubes is low.** CubeCobraML (multi-hot set encoder, 4 heads,
  pool-only input) reaches ~50% top-1 / 83% top-3 on cube picks; authors call it
  "data/capacity-bound". Retail single-set models reach ~68-71%.
* **Deck strength from a draft is very noisy.** Rigaux & Kashima (CoG 2026,
  https://arxiv.org/abs/2607.04782, code github.com/akulen/MtGDraftEncoder): Spearman ~0.20 vs a
  noise ceiling ~0.23 on 17lands deck win rates. Expect a weak, noisy terminal signal.
* RL drafting with a fixed battle agent as reward works in simpler CCGs (Vieira et al., SBGames
  2020, Legends of Code and Magic, PPO), but history-free policies did best there (weak battle
  agent, too few episodes).

## Bots and models

* **CubeCobraML** (github.com/dekkerglen/CubeCobraML; serving in CubeCobra
  `packages/recommenderService`): MultiHotEmbedding(512) -> 256 -> 128 encoder of the POOL;
  heads: cube recommend, draft (pack-masked softmax), deck-build, correlation. ~37k oracle-id
  vocabulary. Loss: CE + 0.1 x symmetric KL between batch-mean human pick histogram and the
  model's ("PickHistogramCCE"), label smoothing 0.1, pack-position weighting, last pick weight 0.
* **CubeCobra deckbuilder** (`packages/utils/src/drafting/deckbuildCore.ts`): build head seeds 10
  cards, then the draft head fills the deck treating the rest of the pool as the pack, 0.9^n
  diminishing returns, then manabase heuristics. No explicit colour step.
* **mtgdraftbots** (CubeArtisan, C++): six weighted oracles (rating, colours, openness, pick /
  internal / external synergy) weighted by castability over 8 land combinations; weights per
  pack x pick.
* **mtgml** (CubeArtisan/mtgml, dev branch): multitask draft/deckbuild/cube model with shared
  embeddings and a transformer; no published accuracy.
* **Draftmancer bots** (src/Bot.ts): private DraftmancerAI per set, else CubeCobra bot endpoint,
  else mtgml, else SimpleBot `rating + 0.35 x pickedColors[color]`.
* **Statistical Drafting** (Brooks): MLP over pool + pack, ~70% agreement with top players.
* **RyanSaxe/mtg**: transformer drafter + iterative deckbuilder trained on 17lands decks;
  Mythic in 23 drafts at 66%.

## Papers (pick prediction)

* Ward et al. 2020 (arXiv 2009.00655): Draftsim M19, NNetBot 48.7% top-1 (pool-only input).
* Bertram et al. 2021 (arXiv 2105.11864): Siamese pool/card embedding, triplet loss, 83.8% on
  Draftsim M19.
* Bertram et al. 2024 (arXiv 2407.05879): ~68% on seen sets regardless of card representation;
  representations matter for unseen sets (55.4% with multi-set pretraining).
* DraftFM (Ward, arXiv 2608.19568): card features + text embedding, no card identity; 68.6%
  in-distribution, 51-60% zero-shot on new sets.
* UrzaGPT (arXiv 2508.08382): LoRA LLMs 66.2% on 17lands logs.

## Other

* No public MTGO Vintage Cube pick dataset. Pick-order article: Ryan Overturf, SCG 2020-07-08.
* Draft logs Draftmancer uploads to CubeArtisan need API keys (not public).
* Unverified: mtgml / mtgdraftbots accuracy, DraftmancerAI internals, "Czerner 2025".
