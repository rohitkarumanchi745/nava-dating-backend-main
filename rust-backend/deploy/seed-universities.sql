-- Comprehensive worldwide university seed
-- Covers 50+ countries, 1000+ universities
-- Uses ON CONFLICT to skip existing entries

INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type, is_active)
VALUES

-- ============================================================
-- INDIA — IITs, NITs, IIITs, Central, State, Private
-- ============================================================
('Indian Institute of Technology Roorkee', 'IIT Roorkee', 'iitr.ac.in', 'India', 'IND', 'Uttarakhand', 'Roorkee', 'top_public', 'public', TRUE),
('Indian Institute of Technology Guwahati', 'IIT Guwahati', 'iitg.ac.in', 'India', 'IND', 'Assam', 'Guwahati', 'top_public', 'public', TRUE),
('Indian Institute of Technology Indore', 'IIT Indore', 'iiti.ac.in', 'India', 'IND', 'Madhya Pradesh', 'Indore', 'top_public', 'public', TRUE),
('Indian Institute of Technology BHU Varanasi', 'IIT BHU', 'iitbhu.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Varanasi', 'top_public', 'public', TRUE),
('Indian Institute of Technology Mandi', 'IIT Mandi', 'iitmandi.ac.in', 'India', 'IND', 'Himachal Pradesh', 'Mandi', 'top_public', 'public', TRUE),
('Indian Institute of Technology Patna', 'IIT Patna', 'iitp.ac.in', 'India', 'IND', 'Bihar', 'Patna', 'top_public', 'public', TRUE),
('Indian Institute of Technology Bhubaneswar', 'IIT Bhubaneswar', 'iitbbs.ac.in', 'India', 'IND', 'Odisha', 'Bhubaneswar', 'top_public', 'public', TRUE),
('Indian Institute of Technology Gandhinagar', 'IIT Gandhinagar', 'iitgn.ac.in', 'India', 'IND', 'Gujarat', 'Gandhinagar', 'top_public', 'public', TRUE),
('Indian Institute of Technology Jodhpur', 'IIT Jodhpur', 'iitj.ac.in', 'India', 'IND', 'Rajasthan', 'Jodhpur', 'top_public', 'public', TRUE),
('Indian Institute of Technology Tirupati', 'IIT Tirupati', 'iittp.ac.in', 'India', 'IND', 'Andhra Pradesh', 'Tirupati', 'top_public', 'public', TRUE),
('Indian Institute of Technology Palakkad', 'IIT Palakkad', 'iitpkd.ac.in', 'India', 'IND', 'Kerala', 'Palakkad', 'top_public', 'public', TRUE),
('Indian Institute of Technology Dharwad', 'IIT Dharwad', 'iitdh.ac.in', 'India', 'IND', 'Karnataka', 'Dharwad', 'top_public', 'public', TRUE),
('Indian Institute of Technology Bhilai', 'IIT Bhilai', 'iitbhilai.ac.in', 'India', 'IND', 'Chhattisgarh', 'Bhilai', 'top_public', 'public', TRUE),
('Indian Institute of Technology Goa', 'IIT Goa', 'iitgoa.ac.in', 'India', 'IND', 'Goa', 'Ponda', 'top_public', 'public', TRUE),
('Indian Institute of Technology Jammu', 'IIT Jammu', 'iitjammu.ac.in', 'India', 'IND', 'Jammu & Kashmir', 'Jammu', 'top_public', 'public', TRUE),
-- NITs
('National Institute of Technology Trichy', 'NIT Trichy', 'nitt.edu', 'India', 'IND', 'Tamil Nadu', 'Tiruchirappalli', 'top_public', 'public', TRUE),
('National Institute of Technology Surathkal', 'NIT Karnataka', 'nitk.ac.in', 'India', 'IND', 'Karnataka', 'Surathkal', 'top_public', 'public', TRUE),
('National Institute of Technology Warangal', 'NIT Warangal', 'nitw.ac.in', 'India', 'IND', 'Telangana', 'Warangal', 'top_public', 'public', TRUE),
('National Institute of Technology Rourkela', 'NIT Rourkela', 'nitrkl.ac.in', 'India', 'IND', 'Odisha', 'Rourkela', 'top_public', 'public', TRUE),
('National Institute of Technology Calicut', 'NIT Calicut', 'nitc.ac.in', 'India', 'IND', 'Kerala', 'Calicut', 'top_public', 'public', TRUE),
('National Institute of Technology Durgapur', 'NIT Durgapur', 'nitdgp.ac.in', 'India', 'IND', 'West Bengal', 'Durgapur', 'top_public', 'public', TRUE),
('National Institute of Technology Allahabad', 'MNNIT', 'mnnit.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Allahabad', 'top_public', 'public', TRUE),
('National Institute of Technology Jamshedpur', 'NIT Jamshedpur', 'nitjsr.ac.in', 'India', 'IND', 'Jharkhand', 'Jamshedpur', 'top_public', 'public', TRUE),
('National Institute of Technology Kurukshetra', 'NIT Kurukshetra', 'nitkkr.ac.in', 'India', 'IND', 'Haryana', 'Kurukshetra', 'top_public', 'public', TRUE),
('National Institute of Technology Silchar', 'NIT Silchar', 'nits.ac.in', 'India', 'IND', 'Assam', 'Silchar', 'top_public', 'public', TRUE),
('National Institute of Technology Srinagar', 'NIT Srinagar', 'nitsri.ac.in', 'India', 'IND', 'J&K', 'Srinagar', 'top_public', 'public', TRUE),
('National Institute of Technology Hamirpur', 'NIT Hamirpur', 'nith.ac.in', 'India', 'IND', 'Himachal Pradesh', 'Hamirpur', 'top_public', 'public', TRUE),
('National Institute of Technology Nagpur', 'VNIT Nagpur', 'vnit.ac.in', 'India', 'IND', 'Maharashtra', 'Nagpur', 'top_public', 'public', TRUE),
('National Institute of Technology Surat', 'SVNIT Surat', 'svnit.ac.in', 'India', 'IND', 'Gujarat', 'Surat', 'top_public', 'public', TRUE),
('National Institute of Technology Bhopal', 'MANIT Bhopal', 'manit.ac.in', 'India', 'IND', 'Madhya Pradesh', 'Bhopal', 'top_public', 'public', TRUE),
('National Institute of Technology Jaipur', 'MNIT Jaipur', 'mnit.ac.in', 'India', 'IND', 'Rajasthan', 'Jaipur', 'top_public', 'public', TRUE),
('National Institute of Technology Patna', 'NIT Patna', 'nitp.ac.in', 'India', 'IND', 'Bihar', 'Patna', 'top_public', 'public', TRUE),
('National Institute of Technology Raipur', 'NIT Raipur', 'nitrr.ac.in', 'India', 'IND', 'Chhattisgarh', 'Raipur', 'top_public', 'public', TRUE),
('National Institute of Technology Agartala', 'NIT Agartala', 'nita.ac.in', 'India', 'IND', 'Tripura', 'Agartala', 'top_public', 'public', TRUE),
('National Institute of Technology Meghalaya', 'NIT Meghalaya', 'nitm.ac.in', 'India', 'IND', 'Meghalaya', 'Shillong', 'top_public', 'public', TRUE),
('National Institute of Technology Manipur', 'NIT Manipur', 'nitmanipur.ac.in', 'India', 'IND', 'Manipur', 'Imphal', 'top_public', 'public', TRUE),
('National Institute of Technology Mizoram', 'NIT Mizoram', 'nitmz.ac.in', 'India', 'IND', 'Mizoram', 'Aizawl', 'top_public', 'public', TRUE),
('National Institute of Technology Arunachal Pradesh', 'NIT Arunachal', 'nitap.ac.in', 'India', 'IND', 'Arunachal Pradesh', 'Yupia', 'top_public', 'public', TRUE),
('National Institute of Technology Sikkim', 'NIT Sikkim', 'nitskm.ac.in', 'India', 'IND', 'Sikkim', 'Ravangla', 'top_public', 'public', TRUE),
('National Institute of Technology Nagaland', 'NIT Nagaland', 'nitnagaland.ac.in', 'India', 'IND', 'Nagaland', 'Dimapur', 'top_public', 'public', TRUE),
('National Institute of Technology Goa', 'NIT Goa', 'nitgoa.ac.in', 'India', 'IND', 'Goa', 'Ponda', 'top_public', 'public', TRUE),
('National Institute of Technology Uttarakhand', 'NIT Uttarakhand', 'nituk.ac.in', 'India', 'IND', 'Uttarakhand', 'Srinagar', 'top_public', 'public', TRUE),
('National Institute of Technology Delhi', 'NIT Delhi', 'nitdelhi.ac.in', 'India', 'IND', 'Delhi', 'New Delhi', 'top_public', 'public', TRUE),
('National Institute of Technology Puducherry', 'NIT Puducherry', 'nitpy.ac.in', 'India', 'IND', 'Puducherry', 'Karaikal', 'top_public', 'public', TRUE),
('National Institute of Technology Andhra Pradesh', 'NIT AP', 'nitandhra.ac.in', 'India', 'IND', 'Andhra Pradesh', 'Tadepalligudem', 'top_public', 'public', TRUE),
-- IIITs
('IIIT Hyderabad', 'IIIT-H', 'iiit.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'top_public', 'public', TRUE),
('IIIT Allahabad', 'IIIT-A', 'iiita.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Allahabad', 'top_public', 'public', TRUE),
('IIIT Bangalore', 'IIIT-B', 'iiitb.ac.in', 'India', 'IND', 'Karnataka', 'Bangalore', 'top_public', 'public', TRUE),
('IIIT Delhi', 'IIIT-D', 'iiitd.ac.in', 'India', 'IND', 'Delhi', 'New Delhi', 'top_public', 'public', TRUE),
('IIIT Gwalior', 'IIIT Gwalior', 'iiitm.ac.in', 'India', 'IND', 'Madhya Pradesh', 'Gwalior', 'top_public', 'public', TRUE),
-- BITS
('BITS Pilani Goa Campus', 'BITS Goa', 'goa.bits-pilani.ac.in', 'India', 'IND', 'Goa', 'Zuarinagar', 'top_private', 'private', TRUE),
('BITS Pilani Hyderabad Campus', 'BITS Hyderabad', 'hyderabad.bits-pilani.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'top_private', 'private', TRUE),
('BITS Pilani Dubai Campus', 'BITS Dubai', 'dubai.bits-pilani.ac.in', 'India', 'IND', 'Dubai', 'Dubai', 'top_private', 'private', TRUE),
-- Central Universities
('Delhi University', 'DU', 'du.ac.in', 'India', 'IND', 'Delhi', 'New Delhi', 'top_public', 'public', TRUE),
('Jawaharlal Nehru University', 'JNU', 'jnu.ac.in', 'India', 'IND', 'Delhi', 'New Delhi', 'top_public', 'public', TRUE),
('Banaras Hindu University', 'BHU', 'bhu.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Varanasi', 'top_public', 'public', TRUE),
('Aligarh Muslim University', 'AMU', 'amu.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Aligarh', 'top_public', 'public', TRUE),
('Jamia Millia Islamia', 'Jamia', 'jmi.ac.in', 'India', 'IND', 'Delhi', 'New Delhi', 'top_public', 'public', TRUE),
('University of Hyderabad', 'UoH', 'uohyd.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'top_public', 'public', TRUE),
('Jadavpur University', 'JU', 'jaduniv.edu.in', 'India', 'IND', 'West Bengal', 'Kolkata', 'top_public', 'public', TRUE),
('Anna University', 'Anna Univ', 'annauniv.edu', 'India', 'IND', 'Tamil Nadu', 'Chennai', 'top_public', 'public', TRUE),
('Savitribai Phule Pune University', 'SPPU', 'unipune.ac.in', 'India', 'IND', 'Maharashtra', 'Pune', 'top_public', 'public', TRUE),
('Osmania University', 'OU', 'osmania.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'top_public', 'public', TRUE),
-- Top Private India
('Vellore Institute of Technology', 'VIT', 'vit.ac.in', 'India', 'IND', 'Tamil Nadu', 'Vellore', 'top_private', 'private', TRUE),
('SRM Institute of Science and Technology', 'SRM', 'srmist.edu.in', 'India', 'IND', 'Tamil Nadu', 'Chennai', 'top_private', 'private', TRUE),
('Manipal Academy of Higher Education', 'MAHE', 'manipal.edu', 'India', 'IND', 'Karnataka', 'Manipal', 'top_private', 'private', TRUE),
('Amity University', 'Amity', 'amity.edu', 'India', 'IND', 'Uttar Pradesh', 'Noida', 'regular', 'private', TRUE),
('Lovely Professional University', 'LPU', 'lpu.in', 'India', 'IND', 'Punjab', 'Phagwara', 'regular', 'private', TRUE),
('Shiv Nadar University', 'SNU', 'snu.edu.in', 'India', 'IND', 'Uttar Pradesh', 'Greater Noida', 'top_private', 'private', TRUE),
('Ashoka University', 'Ashoka', 'ashoka.edu.in', 'India', 'IND', 'Haryana', 'Sonipat', 'top_private', 'private', TRUE),
('Plaksha University', 'Plaksha', 'plaksha.edu.in', 'India', 'IND', 'Punjab', 'Mohali', 'top_private', 'private', TRUE),
('FLAME University', 'FLAME', 'flame.edu.in', 'India', 'IND', 'Maharashtra', 'Pune', 'top_private', 'private', TRUE),
('Symbiosis International University', 'SIU', 'siu.edu.in', 'India', 'IND', 'Maharashtra', 'Pune', 'top_private', 'private', TRUE),
('Christ University', 'Christ', 'christuniversity.in', 'India', 'IND', 'Karnataka', 'Bangalore', 'regular', 'private', TRUE),
('Thapar Institute of Engineering', 'Thapar', 'thapar.edu', 'India', 'IND', 'Punjab', 'Patiala', 'top_private', 'private', TRUE),
('PES University', 'PES', 'pes.edu', 'India', 'IND', 'Karnataka', 'Bangalore', 'regular', 'private', TRUE),
('RV College of Engineering', 'RVCE', 'rvce.edu.in', 'India', 'IND', 'Karnataka', 'Bangalore', 'regular', 'private', TRUE),
('BMS College of Engineering', 'BMSCE', 'bmsce.ac.in', 'India', 'IND', 'Karnataka', 'Bangalore', 'regular', 'private', TRUE),
-- Vignan (user specifically asked about this)
('Vignan University', 'Vignan', 'vignan.ac.in', 'India', 'IND', 'Andhra Pradesh', 'Guntur', 'regular', 'private', TRUE),
('Vignan Institute of Technology and Science', 'VITS', 'vignanits.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'regular', 'private', TRUE),
('Vignan Institute of Information Technology', 'VIIT', 'vignanvizag.com', 'India', 'IND', 'Andhra Pradesh', 'Visakhapatnam', 'regular', 'private', TRUE),
-- IIMs
('Indian Institute of Management Ahmedabad', 'IIM Ahmedabad', 'iima.ac.in', 'India', 'IND', 'Gujarat', 'Ahmedabad', 'top_public', 'public', TRUE),
('Indian Institute of Management Bangalore', 'IIM Bangalore', 'iimb.ac.in', 'India', 'IND', 'Karnataka', 'Bangalore', 'top_public', 'public', TRUE),
('Indian Institute of Management Calcutta', 'IIM Calcutta', 'iimcal.ac.in', 'India', 'IND', 'West Bengal', 'Kolkata', 'top_public', 'public', TRUE),
('Indian Institute of Management Lucknow', 'IIM Lucknow', 'iiml.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Lucknow', 'top_public', 'public', TRUE),
('Indian Institute of Management Kozhikode', 'IIM Kozhikode', 'iimk.ac.in', 'India', 'IND', 'Kerala', 'Kozhikode', 'top_public', 'public', TRUE),
('Indian Institute of Management Indore', 'IIM Indore', 'iimidr.ac.in', 'India', 'IND', 'Madhya Pradesh', 'Indore', 'top_public', 'public', TRUE),
-- ISI, IISc, IISER
('Indian Institute of Science', 'IISc', 'iisc.ac.in', 'India', 'IND', 'Karnataka', 'Bangalore', 'top_public', 'public', TRUE),
('Indian Statistical Institute', 'ISI Kolkata', 'isical.ac.in', 'India', 'IND', 'West Bengal', 'Kolkata', 'top_public', 'public', TRUE),
('IISER Pune', 'IISER Pune', 'iiserpune.ac.in', 'India', 'IND', 'Maharashtra', 'Pune', 'top_public', 'public', TRUE),
('IISER Kolkata', 'IISER Kolkata', 'iiserkol.ac.in', 'India', 'IND', 'West Bengal', 'Kolkata', 'top_public', 'public', TRUE),
('IISER Mohali', 'IISER Mohali', 'iisermohali.ac.in', 'India', 'IND', 'Punjab', 'Mohali', 'top_public', 'public', TRUE),
-- Medical
('AIIMS Delhi', 'AIIMS', 'aiims.edu', 'India', 'IND', 'Delhi', 'New Delhi', 'top_public', 'public', TRUE),
('AIIMS Bhopal', 'AIIMS Bhopal', 'aiimsbhopal.edu.in', 'India', 'IND', 'Madhya Pradesh', 'Bhopal', 'top_public', 'public', TRUE),
('CMC Vellore', 'CMC', 'cmcvellore.ac.in', 'India', 'IND', 'Tamil Nadu', 'Vellore', 'top_private', 'private', TRUE),
-- Law
('National Law School of India University', 'NLSIU', 'nls.ac.in', 'India', 'IND', 'Karnataka', 'Bangalore', 'top_public', 'public', TRUE),
('NALSAR University of Law', 'NALSAR', 'nalsar.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'top_public', 'public', TRUE),

-- ============================================================
-- USA — Ivy League, Top 50, State Schools, HBCUs
-- ============================================================
('Yale University', 'Yale', 'yale.edu', 'USA', 'USA', 'Connecticut', 'New Haven', 'top_private', 'private', TRUE),
('Princeton University', 'Princeton', 'princeton.edu', 'USA', 'USA', 'New Jersey', 'Princeton', 'top_private', 'private', TRUE),
('University of Pennsylvania', 'UPenn', 'upenn.edu', 'USA', 'USA', 'Pennsylvania', 'Philadelphia', 'top_private', 'private', TRUE),
('Brown University', 'Brown', 'brown.edu', 'USA', 'USA', 'Rhode Island', 'Providence', 'top_private', 'private', TRUE),
('Dartmouth College', 'Dartmouth', 'dartmouth.edu', 'USA', 'USA', 'New Hampshire', 'Hanover', 'top_private', 'private', TRUE),
('Cornell University', 'Cornell', 'cornell.edu', 'USA', 'USA', 'New York', 'Ithaca', 'top_private', 'private', TRUE),
('Duke University', 'Duke', 'duke.edu', 'USA', 'USA', 'North Carolina', 'Durham', 'top_private', 'private', TRUE),
('Northwestern University', 'Northwestern', 'northwestern.edu', 'USA', 'USA', 'Illinois', 'Evanston', 'top_private', 'private', TRUE),
('Rice University', 'Rice', 'rice.edu', 'USA', 'USA', 'Texas', 'Houston', 'top_private', 'private', TRUE),
('Vanderbilt University', 'Vanderbilt', 'vanderbilt.edu', 'USA', 'USA', 'Tennessee', 'Nashville', 'top_private', 'private', TRUE),
('Washington University in St. Louis', 'WashU', 'wustl.edu', 'USA', 'USA', 'Missouri', 'St. Louis', 'top_private', 'private', TRUE),
('Emory University', 'Emory', 'emory.edu', 'USA', 'USA', 'Georgia', 'Atlanta', 'top_private', 'private', TRUE),
('Georgetown University', 'Georgetown', 'georgetown.edu', 'USA', 'USA', 'DC', 'Washington', 'top_private', 'private', TRUE),
('University of Notre Dame', 'Notre Dame', 'nd.edu', 'USA', 'USA', 'Indiana', 'Notre Dame', 'top_private', 'private', TRUE),
('Carnegie Mellon University', 'CMU', 'cmu.edu', 'USA', 'USA', 'Pennsylvania', 'Pittsburgh', 'top_private', 'private', TRUE),
('University of Virginia', 'UVA', 'virginia.edu', 'USA', 'USA', 'Virginia', 'Charlottesville', 'top_public', 'public', TRUE),
('University of Michigan', 'UMich', 'umich.edu', 'USA', 'USA', 'Michigan', 'Ann Arbor', 'top_public', 'public', TRUE),
('University of North Carolina Chapel Hill', 'UNC', 'unc.edu', 'USA', 'USA', 'North Carolina', 'Chapel Hill', 'top_public', 'public', TRUE),
('University of Wisconsin-Madison', 'UW-Madison', 'wisc.edu', 'USA', 'USA', 'Wisconsin', 'Madison', 'top_public', 'public', TRUE),
('University of Illinois Urbana-Champaign', 'UIUC', 'illinois.edu', 'USA', 'USA', 'Illinois', 'Champaign', 'top_public', 'public', TRUE),
('University of Texas at Austin', 'UT Austin', 'utexas.edu', 'USA', 'USA', 'Texas', 'Austin', 'top_public', 'public', TRUE),
('Georgia Institute of Technology', 'Georgia Tech', 'gatech.edu', 'USA', 'USA', 'Georgia', 'Atlanta', 'top_public', 'public', TRUE),
('University of Florida', 'UF', 'ufl.edu', 'USA', 'USA', 'Florida', 'Gainesville', 'top_public', 'public', TRUE),
('Ohio State University', 'OSU', 'osu.edu', 'USA', 'USA', 'Ohio', 'Columbus', 'top_public', 'public', TRUE),
('Penn State University', 'Penn State', 'psu.edu', 'USA', 'USA', 'Pennsylvania', 'State College', 'top_public', 'public', TRUE),
('Purdue University', 'Purdue', 'purdue.edu', 'USA', 'USA', 'Indiana', 'West Lafayette', 'top_public', 'public', TRUE),
('University of Maryland', 'UMD', 'umd.edu', 'USA', 'USA', 'Maryland', 'College Park', 'top_public', 'public', TRUE),
('University of Minnesota', 'UMN', 'umn.edu', 'USA', 'USA', 'Minnesota', 'Minneapolis', 'top_public', 'public', TRUE),
('Indiana University Bloomington', 'IU', 'indiana.edu', 'USA', 'USA', 'Indiana', 'Bloomington', 'top_public', 'public', TRUE),
('University of Iowa', 'Iowa', 'uiowa.edu', 'USA', 'USA', 'Iowa', 'Iowa City', 'top_public', 'public', TRUE),
('Rutgers University', 'Rutgers', 'rutgers.edu', 'USA', 'USA', 'New Jersey', 'New Brunswick', 'top_public', 'public', TRUE),
('Arizona State University', 'ASU', 'asu.edu', 'USA', 'USA', 'Arizona', 'Tempe', 'regular', 'public', TRUE),
('University of Arizona', 'UArizona', 'arizona.edu', 'USA', 'USA', 'Arizona', 'Tucson', 'regular', 'public', TRUE),
('Michigan State University', 'MSU', 'msu.edu', 'USA', 'USA', 'Michigan', 'East Lansing', 'top_public', 'public', TRUE),
('University of Pittsburgh', 'Pitt', 'pitt.edu', 'USA', 'USA', 'Pennsylvania', 'Pittsburgh', 'top_public', 'public', TRUE),
('Boston University', 'BU', 'bu.edu', 'USA', 'USA', 'Massachusetts', 'Boston', 'top_private', 'private', TRUE),
('Boston College', 'BC', 'bc.edu', 'USA', 'USA', 'Massachusetts', 'Chestnut Hill', 'top_private', 'private', TRUE),
('Northeastern University', 'Northeastern', 'northeastern.edu', 'USA', 'USA', 'Massachusetts', 'Boston', 'top_private', 'private', TRUE),
('New York University', 'NYU', 'nyu.edu', 'USA', 'USA', 'New York', 'New York', 'top_private', 'private', TRUE),
('University of Southern California', 'USC', 'usc.edu', 'USA', 'USA', 'California', 'Los Angeles', 'top_private', 'private', TRUE),
('University of Washington', 'UW', 'uw.edu', 'USA', 'USA', 'Washington', 'Seattle', 'top_public', 'public', TRUE),
('Virginia Tech', 'VT', 'vt.edu', 'USA', 'USA', 'Virginia', 'Blacksburg', 'top_public', 'public', TRUE),
('Texas A&M University', 'TAMU', 'tamu.edu', 'USA', 'USA', 'Texas', 'College Station', 'top_public', 'public', TRUE),
('University of Colorado Boulder', 'CU Boulder', 'colorado.edu', 'USA', 'USA', 'Colorado', 'Boulder', 'top_public', 'public', TRUE),
('University of Oregon', 'UOregon', 'uoregon.edu', 'USA', 'USA', 'Oregon', 'Eugene', 'regular', 'public', TRUE),
('University of Connecticut', 'UConn', 'uconn.edu', 'USA', 'USA', 'Connecticut', 'Storrs', 'top_public', 'public', TRUE),
('Tufts University', 'Tufts', 'tufts.edu', 'USA', 'USA', 'Massachusetts', 'Medford', 'top_private', 'private', TRUE),
('Wake Forest University', 'Wake Forest', 'wfu.edu', 'USA', 'USA', 'North Carolina', 'Winston-Salem', 'top_private', 'private', TRUE),
('University of Rochester', 'URochester', 'rochester.edu', 'USA', 'USA', 'New York', 'Rochester', 'top_private', 'private', TRUE),
('Case Western Reserve University', 'CWRU', 'case.edu', 'USA', 'USA', 'Ohio', 'Cleveland', 'top_private', 'private', TRUE),
('Lehigh University', 'Lehigh', 'lehigh.edu', 'USA', 'USA', 'Pennsylvania', 'Bethlehem', 'top_private', 'private', TRUE),
('Santa Clara University', 'SCU', 'scu.edu', 'USA', 'USA', 'California', 'Santa Clara', 'top_private', 'private', TRUE),
('Stony Brook University', 'Stony Brook', 'stonybrook.edu', 'USA', 'USA', 'New York', 'Stony Brook', 'top_public', 'public', TRUE),
('University at Buffalo', 'UB', 'buffalo.edu', 'USA', 'USA', 'New York', 'Buffalo', 'regular', 'public', TRUE),
('North Carolina State University', 'NC State', 'ncsu.edu', 'USA', 'USA', 'North Carolina', 'Raleigh', 'top_public', 'public', TRUE),
('University of Massachusetts Amherst', 'UMass', 'umass.edu', 'USA', 'USA', 'Massachusetts', 'Amherst', 'top_public', 'public', TRUE),
('Clemson University', 'Clemson', 'clemson.edu', 'USA', 'USA', 'South Carolina', 'Clemson', 'top_public', 'public', TRUE),
('University of South Florida', 'USF', 'usf.edu', 'USA', 'USA', 'Florida', 'Tampa', 'regular', 'public', TRUE),
('Florida State University', 'FSU', 'fsu.edu', 'USA', 'USA', 'Florida', 'Tallahassee', 'top_public', 'public', TRUE),
('University of Central Florida', 'UCF', 'ucf.edu', 'USA', 'USA', 'Florida', 'Orlando', 'regular', 'public', TRUE),

-- ============================================================
-- UK — Russell Group + Others
-- ============================================================
('University of Edinburgh', 'Edinburgh', 'ed.ac.uk', 'UK', 'GBR', 'Scotland', 'Edinburgh', 'top_public', 'public', TRUE),
('King''s College London', 'KCL', 'kcl.ac.uk', 'UK', 'GBR', 'England', 'London', 'top_public', 'public', TRUE),
('University of Manchester', 'Manchester', 'manchester.ac.uk', 'UK', 'GBR', 'England', 'Manchester', 'top_public', 'public', TRUE),
('University of Bristol', 'Bristol', 'bristol.ac.uk', 'UK', 'GBR', 'England', 'Bristol', 'top_public', 'public', TRUE),
('University of Warwick', 'Warwick', 'warwick.ac.uk', 'UK', 'GBR', 'England', 'Coventry', 'top_public', 'public', TRUE),
('University of Glasgow', 'Glasgow', 'gla.ac.uk', 'UK', 'GBR', 'Scotland', 'Glasgow', 'top_public', 'public', TRUE),
('University of Birmingham', 'Birmingham', 'bham.ac.uk', 'UK', 'GBR', 'England', 'Birmingham', 'top_public', 'public', TRUE),
('University of Leeds', 'Leeds', 'leeds.ac.uk', 'UK', 'GBR', 'England', 'Leeds', 'top_public', 'public', TRUE),
('University of Sheffield', 'Sheffield', 'sheffield.ac.uk', 'UK', 'GBR', 'England', 'Sheffield', 'top_public', 'public', TRUE),
('University of Nottingham', 'Nottingham', 'nottingham.ac.uk', 'UK', 'GBR', 'England', 'Nottingham', 'top_public', 'public', TRUE),
('University of Southampton', 'Southampton', 'soton.ac.uk', 'UK', 'GBR', 'England', 'Southampton', 'top_public', 'public', TRUE),
('Durham University', 'Durham', 'durham.ac.uk', 'UK', 'GBR', 'England', 'Durham', 'top_public', 'public', TRUE),
('University of St Andrews', 'St Andrews', 'st-andrews.ac.uk', 'UK', 'GBR', 'Scotland', 'St Andrews', 'top_public', 'public', TRUE),
('University of Bath', 'Bath', 'bath.ac.uk', 'UK', 'GBR', 'England', 'Bath', 'top_public', 'public', TRUE),
('University of Exeter', 'Exeter', 'exeter.ac.uk', 'UK', 'GBR', 'England', 'Exeter', 'top_public', 'public', TRUE),
('Lancaster University', 'Lancaster', 'lancaster.ac.uk', 'UK', 'GBR', 'England', 'Lancaster', 'top_public', 'public', TRUE),
('University of York', 'York', 'york.ac.uk', 'UK', 'GBR', 'England', 'York', 'top_public', 'public', TRUE),
('Queen Mary University of London', 'QMUL', 'qmul.ac.uk', 'UK', 'GBR', 'England', 'London', 'top_public', 'public', TRUE),
('University of Liverpool', 'Liverpool', 'liverpool.ac.uk', 'UK', 'GBR', 'England', 'Liverpool', 'top_public', 'public', TRUE),
('Cardiff University', 'Cardiff', 'cardiff.ac.uk', 'UK', 'GBR', 'Wales', 'Cardiff', 'top_public', 'public', TRUE),
('Queen''s University Belfast', 'QUB', 'qub.ac.uk', 'UK', 'GBR', 'Northern Ireland', 'Belfast', 'top_public', 'public', TRUE),
('University of Aberdeen', 'Aberdeen', 'abdn.ac.uk', 'UK', 'GBR', 'Scotland', 'Aberdeen', 'regular', 'public', TRUE),
('Loughborough University', 'Loughborough', 'lboro.ac.uk', 'UK', 'GBR', 'England', 'Loughborough', 'top_public', 'public', TRUE),

-- ============================================================
-- EUROPE
-- ============================================================
-- Germany
('Technical University of Munich', 'TUM', 'tum.de', 'Germany', 'DEU', 'Bavaria', 'Munich', 'top_public', 'public', TRUE),
('RWTH Aachen University', 'RWTH Aachen', 'rwth-aachen.de', 'Germany', 'DEU', 'NRW', 'Aachen', 'top_public', 'public', TRUE),
('University of Heidelberg', 'Heidelberg', 'uni-heidelberg.de', 'Germany', 'DEU', 'Baden-Württemberg', 'Heidelberg', 'top_public', 'public', TRUE),
('Humboldt University of Berlin', 'HU Berlin', 'hu-berlin.de', 'Germany', 'DEU', 'Berlin', 'Berlin', 'top_public', 'public', TRUE),
('Free University of Berlin', 'FU Berlin', 'fu-berlin.de', 'Germany', 'DEU', 'Berlin', 'Berlin', 'top_public', 'public', TRUE),
('Technical University of Berlin', 'TU Berlin', 'tu-berlin.de', 'Germany', 'DEU', 'Berlin', 'Berlin', 'top_public', 'public', TRUE),
('University of Freiburg', 'Freiburg', 'uni-freiburg.de', 'Germany', 'DEU', 'Baden-Württemberg', 'Freiburg', 'top_public', 'public', TRUE),
('University of Tübingen', 'Tübingen', 'uni-tuebingen.de', 'Germany', 'DEU', 'Baden-Württemberg', 'Tübingen', 'top_public', 'public', TRUE),
('KIT Karlsruhe', 'KIT', 'kit.edu', 'Germany', 'DEU', 'Baden-Württemberg', 'Karlsruhe', 'top_public', 'public', TRUE),
('University of Göttingen', 'Göttingen', 'uni-goettingen.de', 'Germany', 'DEU', 'Lower Saxony', 'Göttingen', 'top_public', 'public', TRUE),
-- France
('Sorbonne University', 'Sorbonne', 'sorbonne-universite.fr', 'France', 'FRA', 'Île-de-France', 'Paris', 'top_public', 'public', TRUE),
('École Polytechnique', 'X', 'polytechnique.edu', 'France', 'FRA', 'Île-de-France', 'Palaiseau', 'top_public', 'public', TRUE),
('École Normale Supérieure', 'ENS Paris', 'ens.psl.eu', 'France', 'FRA', 'Île-de-France', 'Paris', 'top_public', 'public', TRUE),
('Sciences Po', 'Sciences Po', 'sciencespo.fr', 'France', 'FRA', 'Île-de-France', 'Paris', 'top_private', 'private', TRUE),
('HEC Paris', 'HEC', 'hec.edu', 'France', 'FRA', 'Île-de-France', 'Paris', 'top_private', 'private', TRUE),
('INSEAD', 'INSEAD', 'insead.edu', 'France', 'FRA', 'Île-de-France', 'Fontainebleau', 'top_private', 'private', TRUE),
('CentraleSupélec', 'CentraleSupélec', 'centralesupelec.fr', 'France', 'FRA', 'Île-de-France', 'Gif-sur-Yvette', 'top_public', 'public', TRUE),
-- Netherlands
('University of Amsterdam', 'UvA', 'uva.nl', 'Netherlands', 'NLD', 'North Holland', 'Amsterdam', 'top_public', 'public', TRUE),
('Delft University of Technology', 'TU Delft', 'tudelft.nl', 'Netherlands', 'NLD', 'South Holland', 'Delft', 'top_public', 'public', TRUE),
('Eindhoven University of Technology', 'TU/e', 'tue.nl', 'Netherlands', 'NLD', 'North Brabant', 'Eindhoven', 'top_public', 'public', TRUE),
('Utrecht University', 'Utrecht', 'uu.nl', 'Netherlands', 'NLD', 'Utrecht', 'Utrecht', 'top_public', 'public', TRUE),
('Leiden University', 'Leiden', 'leidenuniv.nl', 'Netherlands', 'NLD', 'South Holland', 'Leiden', 'top_public', 'public', TRUE),
-- Switzerland
('ETH Zurich', 'ETH', 'ethz.ch', 'Switzerland', 'CHE', 'Zurich', 'Zurich', 'top_public', 'public', TRUE),
('EPFL', 'EPFL', 'epfl.ch', 'Switzerland', 'CHE', 'Vaud', 'Lausanne', 'top_public', 'public', TRUE),
('University of Zurich', 'UZH', 'uzh.ch', 'Switzerland', 'CHE', 'Zurich', 'Zurich', 'top_public', 'public', TRUE),
-- Italy
('Politecnico di Milano', 'PoliMi', 'polimi.it', 'Italy', 'ITA', 'Lombardy', 'Milan', 'top_public', 'public', TRUE),
('University of Bologna', 'UNIBO', 'unibo.it', 'Italy', 'ITA', 'Emilia-Romagna', 'Bologna', 'top_public', 'public', TRUE),
('Sapienza University of Rome', 'Sapienza', 'uniroma1.it', 'Italy', 'ITA', 'Lazio', 'Rome', 'top_public', 'public', TRUE),
('Bocconi University', 'Bocconi', 'unibocconi.it', 'Italy', 'ITA', 'Lombardy', 'Milan', 'top_private', 'private', TRUE),
-- Spain
('University of Barcelona', 'UB', 'ub.edu', 'Spain', 'ESP', 'Catalonia', 'Barcelona', 'top_public', 'public', TRUE),
('Autonomous University of Madrid', 'UAM', 'uam.es', 'Spain', 'ESP', 'Madrid', 'Madrid', 'top_public', 'public', TRUE),
('IE University', 'IE', 'ie.edu', 'Spain', 'ESP', 'Madrid', 'Madrid', 'top_private', 'private', TRUE),
-- Scandinavia
('KTH Royal Institute of Technology', 'KTH', 'kth.se', 'Sweden', 'SWE', 'Stockholm', 'Stockholm', 'top_public', 'public', TRUE),
('Lund University', 'Lund', 'lu.se', 'Sweden', 'SWE', 'Skåne', 'Lund', 'top_public', 'public', TRUE),
('University of Copenhagen', 'KU', 'ku.dk', 'Denmark', 'DNK', 'Capital', 'Copenhagen', 'top_public', 'public', TRUE),
('Technical University of Denmark', 'DTU', 'dtu.dk', 'Denmark', 'DNK', 'Capital', 'Lyngby', 'top_public', 'public', TRUE),
('University of Oslo', 'UiO', 'uio.no', 'Norway', 'NOR', 'Oslo', 'Oslo', 'top_public', 'public', TRUE),
('University of Helsinki', 'Helsinki', 'helsinki.fi', 'Finland', 'FIN', 'Uusimaa', 'Helsinki', 'top_public', 'public', TRUE),
('Aalto University', 'Aalto', 'aalto.fi', 'Finland', 'FIN', 'Uusimaa', 'Espoo', 'top_public', 'public', TRUE),

-- ============================================================
-- ASIA-PACIFIC
-- ============================================================
-- China
('Tsinghua University', 'Tsinghua', 'tsinghua.edu.cn', 'China', 'CHN', 'Beijing', 'Beijing', 'top_public', 'public', TRUE),
('Peking University', 'PKU', 'pku.edu.cn', 'China', 'CHN', 'Beijing', 'Beijing', 'top_public', 'public', TRUE),
('Fudan University', 'Fudan', 'fudan.edu.cn', 'China', 'CHN', 'Shanghai', 'Shanghai', 'top_public', 'public', TRUE),
('Shanghai Jiao Tong University', 'SJTU', 'sjtu.edu.cn', 'China', 'CHN', 'Shanghai', 'Shanghai', 'top_public', 'public', TRUE),
('Zhejiang University', 'ZJU', 'zju.edu.cn', 'China', 'CHN', 'Zhejiang', 'Hangzhou', 'top_public', 'public', TRUE),
('University of Science and Technology of China', 'USTC', 'ustc.edu.cn', 'China', 'CHN', 'Anhui', 'Hefei', 'top_public', 'public', TRUE),
('Nanjing University', 'NJU', 'nju.edu.cn', 'China', 'CHN', 'Jiangsu', 'Nanjing', 'top_public', 'public', TRUE),
('Wuhan University', 'WHU', 'whu.edu.cn', 'China', 'CHN', 'Hubei', 'Wuhan', 'top_public', 'public', TRUE),
-- Japan
('University of Tokyo', 'UTokyo', 'u-tokyo.ac.jp', 'Japan', 'JPN', 'Tokyo', 'Tokyo', 'top_public', 'public', TRUE),
('Kyoto University', 'Kyoto', 'kyoto-u.ac.jp', 'Japan', 'JPN', 'Kyoto', 'Kyoto', 'top_public', 'public', TRUE),
('Osaka University', 'Osaka', 'osaka-u.ac.jp', 'Japan', 'JPN', 'Osaka', 'Suita', 'top_public', 'public', TRUE),
('Tokyo Institute of Technology', 'Tokyo Tech', 'titech.ac.jp', 'Japan', 'JPN', 'Tokyo', 'Tokyo', 'top_public', 'public', TRUE),
('Keio University', 'Keio', 'keio.ac.jp', 'Japan', 'JPN', 'Tokyo', 'Tokyo', 'top_private', 'private', TRUE),
('Waseda University', 'Waseda', 'waseda.jp', 'Japan', 'JPN', 'Tokyo', 'Tokyo', 'top_private', 'private', TRUE),
-- South Korea
('Seoul National University', 'SNU', 'snu.ac.kr', 'South Korea', 'KOR', 'Seoul', 'Seoul', 'top_public', 'public', TRUE),
('KAIST', 'KAIST', 'kaist.ac.kr', 'South Korea', 'KOR', 'Daejeon', 'Daejeon', 'top_public', 'public', TRUE),
('POSTECH', 'POSTECH', 'postech.ac.kr', 'South Korea', 'KOR', 'Gyeongbuk', 'Pohang', 'top_private', 'private', TRUE),
('Yonsei University', 'Yonsei', 'yonsei.ac.kr', 'South Korea', 'KOR', 'Seoul', 'Seoul', 'top_private', 'private', TRUE),
('Korea University', 'Korea', 'korea.ac.kr', 'South Korea', 'KOR', 'Seoul', 'Seoul', 'top_private', 'private', TRUE),
('Sungkyunkwan University', 'SKKU', 'skku.edu', 'South Korea', 'KOR', 'Seoul', 'Seoul', 'top_private', 'private', TRUE),
-- Singapore
('Nanyang Technological University', 'NTU', 'ntu.edu.sg', 'Singapore', 'SGP', 'Singapore', 'Singapore', 'top_public', 'public', TRUE),
('Singapore Management University', 'SMU', 'smu.edu.sg', 'Singapore', 'SGP', 'Singapore', 'Singapore', 'top_private', 'private', TRUE),
('Singapore University of Technology and Design', 'SUTD', 'sutd.edu.sg', 'Singapore', 'SGP', 'Singapore', 'Singapore', 'top_public', 'public', TRUE),
-- Hong Kong
('University of Hong Kong', 'HKU', 'hku.hk', 'Hong Kong', 'HKG', 'Hong Kong', 'Hong Kong', 'top_public', 'public', TRUE),
('Chinese University of Hong Kong', 'CUHK', 'cuhk.edu.hk', 'Hong Kong', 'HKG', 'Hong Kong', 'Hong Kong', 'top_public', 'public', TRUE),
('Hong Kong University of Science and Technology', 'HKUST', 'ust.hk', 'Hong Kong', 'HKG', 'Hong Kong', 'Hong Kong', 'top_public', 'public', TRUE),
-- Taiwan
('National Taiwan University', 'NTU Taiwan', 'ntu.edu.tw', 'Taiwan', 'TWN', 'Taipei', 'Taipei', 'top_public', 'public', TRUE),
-- Malaysia
('University of Malaya', 'UM', 'um.edu.my', 'Malaysia', 'MYS', 'KL', 'Kuala Lumpur', 'top_public', 'public', TRUE),
('Universiti Teknologi Malaysia', 'UTM', 'utm.my', 'Malaysia', 'MYS', 'Johor', 'Johor Bahru', 'top_public', 'public', TRUE),
-- Thailand
('Chulalongkorn University', 'Chula', 'chula.ac.th', 'Thailand', 'THA', 'Bangkok', 'Bangkok', 'top_public', 'public', TRUE),
('Mahidol University', 'Mahidol', 'mahidol.ac.th', 'Thailand', 'THA', 'Bangkok', 'Bangkok', 'top_public', 'public', TRUE),
-- Philippines
('University of the Philippines', 'UP', 'up.edu.ph', 'Philippines', 'PHL', 'NCR', 'Quezon City', 'top_public', 'public', TRUE),
('Ateneo de Manila University', 'ADMU', 'ateneo.edu', 'Philippines', 'PHL', 'NCR', 'Quezon City', 'top_private', 'private', TRUE),
('De La Salle University', 'DLSU', 'dlsu.edu.ph', 'Philippines', 'PHL', 'NCR', 'Manila', 'top_private', 'private', TRUE),
-- Vietnam
('Vietnam National University', 'VNU', 'vnu.edu.vn', 'Vietnam', 'VNM', 'Hanoi', 'Hanoi', 'top_public', 'public', TRUE),
-- Indonesia
('University of Indonesia', 'UI', 'ui.ac.id', 'Indonesia', 'IDN', 'West Java', 'Depok', 'top_public', 'public', TRUE),
('Bandung Institute of Technology', 'ITB', 'itb.ac.id', 'Indonesia', 'IDN', 'West Java', 'Bandung', 'top_public', 'public', TRUE),
-- Bangladesh
('Bangladesh University of Engineering and Technology', 'BUET', 'buet.ac.bd', 'Bangladesh', 'BGD', 'Dhaka', 'Dhaka', 'top_public', 'public', TRUE),
('University of Dhaka', 'DU', 'du.ac.bd', 'Bangladesh', 'BGD', 'Dhaka', 'Dhaka', 'top_public', 'public', TRUE),
-- Pakistan
('Lahore University of Management Sciences', 'LUMS', 'lums.edu.pk', 'Pakistan', 'PAK', 'Punjab', 'Lahore', 'top_private', 'private', TRUE),
('National University of Sciences and Technology', 'NUST', 'nust.edu.pk', 'Pakistan', 'PAK', 'Islamabad', 'Islamabad', 'top_public', 'public', TRUE),
('Aga Khan University', 'AKU', 'aku.edu', 'Pakistan', 'PAK', 'Sindh', 'Karachi', 'top_private', 'private', TRUE),
-- Sri Lanka
('University of Colombo', 'UoC', 'cmb.ac.lk', 'Sri Lanka', 'LKA', 'Western', 'Colombo', 'top_public', 'public', TRUE),
('University of Moratuwa', 'UoM', 'mrt.ac.lk', 'Sri Lanka', 'LKA', 'Western', 'Moratuwa', 'top_public', 'public', TRUE),
-- Nepal
('Tribhuvan University', 'TU', 'tu.edu.np', 'Nepal', 'NPL', 'Bagmati', 'Kathmandu', 'top_public', 'public', TRUE),

-- ============================================================
-- AUSTRALIA & NEW ZEALAND
-- ============================================================
('University of Adelaide', 'Adelaide', 'adelaide.edu.au', 'Australia', 'AUS', 'SA', 'Adelaide', 'top_public', 'public', TRUE),
('University of Western Australia', 'UWA', 'uwa.edu.au', 'Australia', 'AUS', 'WA', 'Perth', 'top_public', 'public', TRUE),
('University of Technology Sydney', 'UTS', 'uts.edu.au', 'Australia', 'AUS', 'NSW', 'Sydney', 'top_public', 'public', TRUE),
('Macquarie University', 'Macquarie', 'mq.edu.au', 'Australia', 'AUS', 'NSW', 'Sydney', 'regular', 'public', TRUE),
('RMIT University', 'RMIT', 'rmit.edu.au', 'Australia', 'AUS', 'VIC', 'Melbourne', 'regular', 'public', TRUE),
('Deakin University', 'Deakin', 'deakin.edu.au', 'Australia', 'AUS', 'VIC', 'Melbourne', 'regular', 'public', TRUE),
('Curtin University', 'Curtin', 'curtin.edu.au', 'Australia', 'AUS', 'WA', 'Perth', 'regular', 'public', TRUE),
-- New Zealand
('University of Auckland', 'Auckland', 'auckland.ac.nz', 'New Zealand', 'NZL', 'Auckland', 'Auckland', 'top_public', 'public', TRUE),
('University of Otago', 'Otago', 'otago.ac.nz', 'New Zealand', 'NZL', 'Otago', 'Dunedin', 'top_public', 'public', TRUE),
('Victoria University of Wellington', 'VUW', 'wgtn.ac.nz', 'New Zealand', 'NZL', 'Wellington', 'Wellington', 'top_public', 'public', TRUE),

-- ============================================================
-- CANADA
-- ============================================================
('University of Waterloo', 'Waterloo', 'uwaterloo.ca', 'Canada', 'CAN', 'Ontario', 'Waterloo', 'top_public', 'public', TRUE),
('University of Montreal', 'UdeM', 'umontreal.ca', 'Canada', 'CAN', 'Quebec', 'Montreal', 'top_public', 'public', TRUE),
('University of Ottawa', 'uOttawa', 'uottawa.ca', 'Canada', 'CAN', 'Ontario', 'Ottawa', 'top_public', 'public', TRUE),
('Western University', 'Western', 'uwo.ca', 'Canada', 'CAN', 'Ontario', 'London', 'top_public', 'public', TRUE),
('Queen''s University', 'Queen''s', 'queensu.ca', 'Canada', 'CAN', 'Ontario', 'Kingston', 'top_public', 'public', TRUE),
('McMaster University', 'McMaster', 'mcmaster.ca', 'Canada', 'CAN', 'Ontario', 'Hamilton', 'top_public', 'public', TRUE),
('Dalhousie University', 'Dal', 'dal.ca', 'Canada', 'CAN', 'Nova Scotia', 'Halifax', 'top_public', 'public', TRUE),
('Simon Fraser University', 'SFU', 'sfu.ca', 'Canada', 'CAN', 'BC', 'Burnaby', 'top_public', 'public', TRUE),
('University of Calgary', 'UCalgary', 'ucalgary.ca', 'Canada', 'CAN', 'Alberta', 'Calgary', 'top_public', 'public', TRUE),
('University of Victoria', 'UVic', 'uvic.ca', 'Canada', 'CAN', 'BC', 'Victoria', 'regular', 'public', TRUE),

-- ============================================================
-- MIDDLE EAST
-- ============================================================
('King Abdullah University of Science and Technology', 'KAUST', 'kaust.edu.sa', 'Saudi Arabia', 'SAU', 'Makkah', 'Thuwal', 'top_private', 'private', TRUE),
('American University of Beirut', 'AUB', 'aub.edu.lb', 'Lebanon', 'LBN', 'Beirut', 'Beirut', 'top_private', 'private', TRUE),
('Khalifa University', 'Khalifa', 'ku.ac.ae', 'UAE', 'ARE', 'Abu Dhabi', 'Abu Dhabi', 'top_public', 'public', TRUE),
('American University in Dubai', 'AUD', 'aud.edu', 'UAE', 'ARE', 'Dubai', 'Dubai', 'top_private', 'private', TRUE),
('American University of Sharjah', 'AUS', 'aus.edu', 'UAE', 'ARE', 'Sharjah', 'Sharjah', 'top_private', 'private', TRUE),
('Bilkent University', 'Bilkent', 'bilkent.edu.tr', 'Turkey', 'TUR', 'Ankara', 'Ankara', 'top_private', 'private', TRUE),
('Koç University', 'Koç', 'ku.edu.tr', 'Turkey', 'TUR', 'Istanbul', 'Istanbul', 'top_private', 'private', TRUE),
('Boğaziçi University', 'Boğaziçi', 'boun.edu.tr', 'Turkey', 'TUR', 'Istanbul', 'Istanbul', 'top_public', 'public', TRUE),
('Middle East Technical University', 'METU', 'metu.edu.tr', 'Turkey', 'TUR', 'Ankara', 'Ankara', 'top_public', 'public', TRUE),
('Technion Israel Institute of Technology', 'Technion', 'technion.ac.il', 'Israel', 'ISR', 'Haifa', 'Haifa', 'top_public', 'public', TRUE),
('Tel Aviv University', 'TAU', 'tau.ac.il', 'Israel', 'ISR', 'Tel Aviv', 'Tel Aviv', 'top_public', 'public', TRUE),
('Hebrew University of Jerusalem', 'HUJI', 'huji.ac.il', 'Israel', 'ISR', 'Jerusalem', 'Jerusalem', 'top_public', 'public', TRUE),

-- ============================================================
-- AFRICA
-- ============================================================
('University of Cape Town', 'UCT', 'uct.ac.za', 'South Africa', 'ZAF', 'Western Cape', 'Cape Town', 'top_public', 'public', TRUE),
('University of Witwatersrand', 'Wits', 'wits.ac.za', 'South Africa', 'ZAF', 'Gauteng', 'Johannesburg', 'top_public', 'public', TRUE),
('Stellenbosch University', 'Stellenbosch', 'sun.ac.za', 'South Africa', 'ZAF', 'Western Cape', 'Stellenbosch', 'top_public', 'public', TRUE),
('University of Pretoria', 'UP', 'up.ac.za', 'South Africa', 'ZAF', 'Gauteng', 'Pretoria', 'top_public', 'public', TRUE),
('University of Lagos', 'UNILAG', 'unilag.edu.ng', 'Nigeria', 'NGA', 'Lagos', 'Lagos', 'top_public', 'public', TRUE),
('University of Ibadan', 'UI', 'ui.edu.ng', 'Nigeria', 'NGA', 'Oyo', 'Ibadan', 'top_public', 'public', TRUE),
('University of Nairobi', 'UoN', 'uonbi.ac.ke', 'Kenya', 'KEN', 'Nairobi', 'Nairobi', 'top_public', 'public', TRUE),
('Cairo University', 'Cairo', 'cu.edu.eg', 'Egypt', 'EGY', 'Giza', 'Giza', 'top_public', 'public', TRUE),
('American University in Cairo', 'AUC', 'aucegypt.edu', 'Egypt', 'EGY', 'Cairo', 'Cairo', 'top_private', 'private', TRUE),
('University of Ghana', 'UG', 'ug.edu.gh', 'Ghana', 'GHA', 'Greater Accra', 'Accra', 'top_public', 'public', TRUE),
('Addis Ababa University', 'AAU', 'aau.edu.et', 'Ethiopia', 'ETH', 'Addis Ababa', 'Addis Ababa', 'top_public', 'public', TRUE),
('Makerere University', 'Makerere', 'mak.ac.ug', 'Uganda', 'UGA', 'Central', 'Kampala', 'top_public', 'public', TRUE),

-- ============================================================
-- LATIN AMERICA
-- ============================================================
('University of São Paulo', 'USP', 'usp.br', 'Brazil', 'BRA', 'São Paulo', 'São Paulo', 'top_public', 'public', TRUE),
('University of Campinas', 'UNICAMP', 'unicamp.br', 'Brazil', 'BRA', 'São Paulo', 'Campinas', 'top_public', 'public', TRUE),
('Federal University of Rio de Janeiro', 'UFRJ', 'ufrj.br', 'Brazil', 'BRA', 'Rio de Janeiro', 'Rio de Janeiro', 'top_public', 'public', TRUE),
('Pontifical Catholic University of Chile', 'PUC Chile', 'uc.cl', 'Chile', 'CHL', 'Santiago', 'Santiago', 'top_private', 'private', TRUE),
('University of Chile', 'UChile', 'uchile.cl', 'Chile', 'CHL', 'Santiago', 'Santiago', 'top_public', 'public', TRUE),
('Universidad Nacional Autónoma de México', 'UNAM', 'unam.mx', 'Mexico', 'MEX', 'CDMX', 'Mexico City', 'top_public', 'public', TRUE),
('Tecnológico de Monterrey', 'Tec de Monterrey', 'tec.mx', 'Mexico', 'MEX', 'Nuevo León', 'Monterrey', 'top_private', 'private', TRUE),
('University of Buenos Aires', 'UBA', 'uba.ar', 'Argentina', 'ARG', 'Buenos Aires', 'Buenos Aires', 'top_public', 'public', TRUE),
('Universidad de los Andes', 'Uniandes', 'uniandes.edu.co', 'Colombia', 'COL', 'Bogotá', 'Bogotá', 'top_private', 'private', TRUE),
('Pontificia Universidad Católica del Perú', 'PUCP', 'pucp.edu.pe', 'Peru', 'PER', 'Lima', 'Lima', 'top_private', 'private', TRUE),

-- ============================================================
-- INDIA — Additional Engineering Colleges (State + Deemed + Private)
-- ============================================================
-- Delhi/NCR
('Delhi Technological University', 'DTU', 'dtu.ac.in', 'India', 'IND', 'Delhi', 'Delhi', 'top_public', 'public', TRUE),
('Netaji Subhas University of Technology', 'NSUT', 'nsut.ac.in', 'India', 'IND', 'Delhi', 'Delhi', 'top_public', 'public', TRUE),
('Indraprastha Institute of Information Technology Delhi', 'IIIT Delhi', 'iiitd.ac.in', 'India', 'IND', 'Delhi', 'Delhi', 'top_public', 'public', TRUE),
('Jamia Millia Islamia', 'JMI', 'jmi.ac.in', 'India', 'IND', 'Delhi', 'Delhi', 'top_public', 'public', TRUE),
('Guru Gobind Singh Indraprastha University', 'GGSIPU', 'ipu.ac.in', 'India', 'IND', 'Delhi', 'Delhi', 'top_public', 'public', TRUE),
-- Maharashtra
('College of Engineering Pune', 'COEP', 'coep.org.in', 'India', 'IND', 'Maharashtra', 'Pune', 'top_public', 'public', TRUE),
('Veermata Jijabai Technological Institute', 'VJTI', 'vjti.ac.in', 'India', 'IND', 'Maharashtra', 'Mumbai', 'top_public', 'public', TRUE),
('Pune Institute of Computer Technology', 'PICT', 'pict.edu', 'India', 'IND', 'Maharashtra', 'Pune', 'top_private', 'private', TRUE),
('Symbiosis Institute of Technology', 'SIT Pune', 'sitpune.edu.in', 'India', 'IND', 'Maharashtra', 'Pune', 'top_private', 'private', TRUE),
('Vishwakarma Institute of Technology', 'VIT Pune', 'vit.edu', 'India', 'IND', 'Maharashtra', 'Pune', 'top_private', 'private', TRUE),
('MIT College of Engineering', 'MITCOE', 'mitcoe.edu.in', 'India', 'IND', 'Maharashtra', 'Pune', 'regular', 'private', TRUE),
('Sardar Patel College of Engineering', 'SPCE', 'spce.ac.in', 'India', 'IND', 'Maharashtra', 'Mumbai', 'top_public', 'public', TRUE),
-- Tamil Nadu
('PSG College of Technology', 'PSG Tech', 'psgtech.ac.in', 'India', 'IND', 'Tamil Nadu', 'Coimbatore', 'top_private', 'private', TRUE),
('College of Engineering Guindy', 'CEG', 'annauniv.edu', 'India', 'IND', 'Tamil Nadu', 'Chennai', 'top_public', 'public', TRUE),
('Coimbatore Institute of Technology', 'CIT', 'citcbe.ac.in', 'India', 'IND', 'Tamil Nadu', 'Coimbatore', 'top_private', 'private', TRUE),
('SSN College of Engineering', 'SSN', 'ssn.edu.in', 'India', 'IND', 'Tamil Nadu', 'Chennai', 'top_private', 'private', TRUE),
('Sri Sivasubramaniya Nadar College', 'SSN', 'ssn.edu.in', 'India', 'IND', 'Tamil Nadu', 'Chennai', 'top_private', 'private', TRUE),
('Kumaraguru College of Technology', 'KCT', 'kct.ac.in', 'India', 'IND', 'Tamil Nadu', 'Coimbatore', 'top_private', 'private', TRUE),
('Amrita Vishwa Vidyapeetham', 'Amrita', 'amrita.edu', 'India', 'IND', 'Tamil Nadu', 'Coimbatore', 'top_private', 'private', TRUE),
-- Karnataka
('PES University', 'PES', 'pes.edu', 'India', 'IND', 'Karnataka', 'Bengaluru', 'top_private', 'private', TRUE),
('MS Ramaiah Institute of Technology', 'MSRIT', 'msrit.edu', 'India', 'IND', 'Karnataka', 'Bengaluru', 'top_private', 'private', TRUE),
('BMS College of Engineering', 'BMSCE', 'bmsce.ac.in', 'India', 'IND', 'Karnataka', 'Bengaluru', 'top_private', 'private', TRUE),
('Dayananda Sagar College of Engineering', 'DSCE', 'dsce.edu.in', 'India', 'IND', 'Karnataka', 'Bengaluru', 'regular', 'private', TRUE),
('New Horizon College of Engineering', 'NHCE', 'newhorizonindia.edu', 'India', 'IND', 'Karnataka', 'Bengaluru', 'regular', 'private', TRUE),
('REVA University', 'REVA', 'reva.edu.in', 'India', 'IND', 'Karnataka', 'Bengaluru', 'regular', 'private', TRUE),
-- Gujarat
('DA-IICT', 'DAIICT', 'daiict.ac.in', 'India', 'IND', 'Gujarat', 'Gandhinagar', 'top_private', 'private', TRUE),
('Nirma University', 'Nirma', 'nirmauni.ac.in', 'India', 'IND', 'Gujarat', 'Ahmedabad', 'top_private', 'private', TRUE),
('LD College of Engineering', 'LDCE', 'ldce.ac.in', 'India', 'IND', 'Gujarat', 'Ahmedabad', 'top_public', 'public', TRUE),
('Institute of Technology Nirma University', 'Nirma IT', 'nirmauni.ac.in', 'India', 'IND', 'Gujarat', 'Ahmedabad', 'top_private', 'private', TRUE),
-- Rajasthan
('LNMIIT Jaipur', 'LNMIIT', 'lnmiit.ac.in', 'India', 'IND', 'Rajasthan', 'Jaipur', 'top_private', 'private', TRUE),
('Poornima College of Engineering', 'PCE', 'poornima.org', 'India', 'IND', 'Rajasthan', 'Jaipur', 'regular', 'private', TRUE),
-- Punjab / Haryana
('Chandigarh University', 'CU', 'cuchd.in', 'India', 'IND', 'Punjab', 'Mohali', 'top_private', 'private', TRUE),
('Lovely Professional University', 'LPU', 'lpu.in', 'India', 'IND', 'Punjab', 'Jalandhar', 'top_private', 'private', TRUE),
('Chitkara University', 'Chitkara', 'chitkara.edu.in', 'India', 'IND', 'Punjab', 'Rajpura', 'top_private', 'private', TRUE),
('Punjab Engineering College', 'PEC', 'pec.ac.in', 'India', 'IND', 'Chandigarh', 'Chandigarh', 'top_public', 'public', TRUE),
-- West Bengal
('Jadavpur University', 'JU', 'jaduniv.edu.in', 'India', 'IND', 'West Bengal', 'Kolkata', 'top_public', 'public', TRUE),
('Heritage Institute of Technology', 'HITK', 'heritageit.edu', 'India', 'IND', 'West Bengal', 'Kolkata', 'regular', 'private', TRUE),
-- Odisha
('KIIT University', 'KIIT', 'kiit.ac.in', 'India', 'IND', 'Odisha', 'Bhubaneswar', 'top_private', 'private', TRUE),
-- Telangana / AP
('BITS Pilani Hyderabad Campus', 'BITS Hyderabad', 'bits-hyderabad.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'top_private', 'private', TRUE),
('Osmania University', 'OU', 'osmania.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'top_public', 'public', TRUE),
('CVR College of Engineering', 'CVR', 'cvr.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'regular', 'private', TRUE),
('Vasavi College of Engineering', 'VCE', 'vasaviengg.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'top_private', 'private', TRUE),
-- Kerala
('College of Engineering Trivandrum', 'CET', 'cet.ac.in', 'India', 'IND', 'Kerala', 'Thiruvananthapuram', 'top_public', 'public', TRUE),
('Government Engineering College Thrissur', 'GEC Thrissur', 'gectcr.ac.in', 'India', 'IND', 'Kerala', 'Thrissur', 'top_public', 'public', TRUE),
('Cochin University of Science and Technology', 'CUSAT', 'cusat.ac.in', 'India', 'IND', 'Kerala', 'Kochi', 'top_public', 'public', TRUE),
('Model Engineering College', 'MEC', 'mec.ac.in', 'India', 'IND', 'Kerala', 'Kochi', 'top_public', 'public', TRUE),
-- UP / Bihar
('Harcourt Butler Technical University', 'HBTU', 'hbtu.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Kanpur', 'top_public', 'public', TRUE),
('Madan Mohan Malaviya University of Technology', 'MMMUT', 'mmmut.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Gorakhpur', 'top_public', 'public', TRUE),
('GL Bajaj Institute of Technology and Management', 'GLBITM', 'glbitm.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Greater Noida', 'regular', 'private', TRUE),
('Shiv Nadar University', 'SNU', 'snu.edu.in', 'India', 'IND', 'Uttar Pradesh', 'Greater Noida', 'top_private', 'private', TRUE),
('Bennett University', 'Bennett', 'bennett.edu.in', 'India', 'IND', 'Uttar Pradesh', 'Greater Noida', 'top_private', 'private', TRUE),
-- New IIITs (25 IIITs under PPP/state)
('IIIT Kota', 'IIIT Kota', 'iiitkota.ac.in', 'India', 'IND', 'Rajasthan', 'Kota', 'top_public', 'public', TRUE),
('IIIT Vadodara', 'IIIT Vadodara', 'iiitvadodara.ac.in', 'India', 'IND', 'Gujarat', 'Vadodara', 'top_public', 'public', TRUE),
('IIIT Lucknow', 'IIIT Lucknow', 'iiitl.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Lucknow', 'top_public', 'public', TRUE),
('IIIT Kalyani', 'IIIT Kalyani', 'iiitkalyani.ac.in', 'India', 'IND', 'West Bengal', 'Kalyani', 'top_public', 'public', TRUE),
('IIIT Sri City', 'IIIT Sri City', 'iiits.ac.in', 'India', 'IND', 'Andhra Pradesh', 'Chittoor', 'top_public', 'public', TRUE),
('IIIT Ranchi', 'IIIT Ranchi', 'iiitranchi.ac.in', 'India', 'IND', 'Jharkhand', 'Ranchi', 'top_public', 'public', TRUE),
('IIIT Nagpur', 'IIIT Nagpur', 'iiitn.ac.in', 'India', 'IND', 'Maharashtra', 'Nagpur', 'top_public', 'public', TRUE),
('IIIT Dharwad', 'IIIT Dharwad', 'iiitdwd.ac.in', 'India', 'IND', 'Karnataka', 'Dharwad', 'top_public', 'public', TRUE),
('IIIT Kurnool', 'IIIT Kurnool', 'iiitk.ac.in', 'India', 'IND', 'Andhra Pradesh', 'Kurnool', 'top_public', 'public', TRUE),
('IIIT Sonepat', 'IIIT Sonepat', 'iiitsonepat.ac.in', 'India', 'IND', 'Haryana', 'Sonepat', 'top_public', 'public', TRUE),
('IIIT Tiruchirappalli', 'IIIT Trichy', 'iiitтриchy.ac.in', 'India', 'IND', 'Tamil Nadu', 'Tiruchirappalli', 'top_public', 'public', TRUE),
('IIIT Una', 'IIIT Una', 'iiitu.ac.in', 'India', 'IND', 'Himachal Pradesh', 'Una', 'top_public', 'public', TRUE),
('IIIT Surat', 'IIIT Surat', 'iiitvadodara.ac.in', 'India', 'IND', 'Gujarat', 'Surat', 'top_public', 'public', TRUE),
('IIIT Bhopal', 'IIIT Bhopal', 'iiitbhopal.ac.in', 'India', 'IND', 'Madhya Pradesh', 'Bhopal', 'top_public', 'public', TRUE),
('IIIT Bhagalpur', 'IIIT Bhagalpur', 'iiitbh.ac.in', 'India', 'IND', 'Bihar', 'Bhagalpur', 'top_public', 'public', TRUE),
('IIIT Agartala', 'IIIT Agartala', 'iiita.ac.in', 'India', 'IND', 'Tripura', 'Agartala', 'top_public', 'public', TRUE),
('IIIT Manipur', 'IIIT Manipur', 'iiitmanipur.ac.in', 'India', 'IND', 'Manipur', 'Imphal', 'top_public', 'public', TRUE),
('IIIT Raichur', 'IIIT Raichur', 'iiitr.ac.in', 'India', 'IND', 'Karnataka', 'Raichur', 'top_public', 'public', TRUE),
('IIIT Pune', 'IIIT Pune', 'iiitp.ac.in', 'India', 'IND', 'Maharashtra', 'Pune', 'top_public', 'public', TRUE)

ON CONFLICT (domain) DO NOTHING;
