-- AI provider configuration
CREATE TABLE IF NOT EXISTS ai_config (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    provider VARCHAR(50) NOT NULL DEFAULT 'ollama',
    api_url VARCHAR(500) NOT NULL DEFAULT 'http://localhost:11434',
    api_key TEXT,
    model VARCHAR(255) NOT NULL DEFAULT 'llama3.2',
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Lead extractions from WhatsApp conversations
CREATE TABLE IF NOT EXISTS lead_extractions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    phone_number VARCHAR(50) NOT NULL,
    contact_id UUID REFERENCES contacts(id) ON DELETE SET NULL,
    raw_messages TEXT NOT NULL,
    extracted_data JSONB NOT NULL DEFAULT '{}',
    prompt_tokens INT DEFAULT 0,
    completion_tokens INT DEFAULT 0,
    status VARCHAR(50) DEFAULT 'pending',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_lead_extractions_phone ON lead_extractions(phone_number);
CREATE INDEX IF NOT EXISTS idx_lead_extractions_contact ON lead_extractions(contact_id);
