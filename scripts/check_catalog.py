#!/usr/bin/env python3
"""Check the maintained Go catalog without credentials or network access."""
import json,pathlib
root=pathlib.Path(__file__).resolve().parents[1];models=json.loads((root/'config/models.json').read_text());names=set()
for m in models:
 assert m['upstream_id'] and '/' not in m['upstream_id']
 assert m['context_length']>0 and m['max_completion_tokens']>0
 assert set(m['output_modalities'])=={'text'}
 for alias in [m['id'],*m['aliases']]:assert alias not in names,alias;names.add(alias)
 for p in ['prompt','completion']:assert float(m['pricing'][p])>=0
print(f'Validated {len(models)} Go models and {len(names)} unique IDs/aliases.')
