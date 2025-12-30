from pydantic import BaseModel
from typing import Optional
from datetime import date

class UpdateProfileRequest(BaseModel):
    name: Optional[str]
    dob: Optional[date]
    gender: Optional[str]
    profile_photo_url: Optional[str]
